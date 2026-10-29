//! Project memory with provenance. FTS retrieval is local-first; no embeddings
//! are required and no repository content is sent externally during ingestion.

use crate::ai::RouterHandle;
use crate::domain::PermissionPolicy;
use crate::domain::ProviderConfig;
use crate::domain::{DataOrigin, Issue, KnowledgeRecord, PullRequest, Repository};
use crate::error::{GitPilotError, Result};
use crate::store::Store;
use chrono::Utc;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub const MAX_INGEST_BYTES: u64 = 2 * 1024 * 1024;
pub const MAX_CODE_FILES: usize = 120;

#[derive(Clone)]
pub struct KnowledgeService {
    store: Store,
}

#[derive(Debug, Clone)]
pub struct RetrievalHit {
    pub record: KnowledgeRecord,
    pub score: f64,
}

#[derive(Debug, Clone, Default)]
pub struct HybridRetrieval {
    pub hits: Vec<RetrievalHit>,
    pub vector_used: bool,
    pub embeddings_indexed: usize,
    pub missing: Vec<String>,
}

impl KnowledgeService {
    pub fn new(store: Store) -> Self {
        Self { store }
    }

    pub async fn ingest_pull_request(&self, pr: &PullRequest) -> Result<()> {
        if pr.body.trim().is_empty() {
            return Ok(());
        }
        self.store
            .save_knowledge(&KnowledgeRecord {
                id: Uuid::new_v4().to_string(),
                repository_id: Some(pr.repository_id.clone()),
                kind: "pull_request".into(),
                title: format!("#{} {}", pr.number, pr.title),
                content: pr.body.clone(),
                source_object: format!("pr:{}", pr.id),
                source_version: Some(pr.head_sha.clone()),
                source_url: Some(pr.html_url.clone()),
                confidence: 1.0,
                extraction_method: "PR description".into(),
                explicit_fact: true,
                superseded_by: None,
                extracted_at: Utc::now(),
                observed_at: pr.updated_at,
            })
            .await
    }

    pub async fn ingest_issue(&self, issue: &Issue) -> Result<()> {
        if issue.body.trim().is_empty() {
            return Ok(());
        }
        self.store
            .save_knowledge(&KnowledgeRecord {
                id: Uuid::new_v4().to_string(),
                repository_id: Some(issue.repository_id.clone()),
                kind: "issue".into(),
                title: format!("#{} {}", issue.number, issue.title),
                content: issue.body.clone(),
                source_object: format!("issue:{}", issue.id),
                source_version: Some(issue.updated_at.to_rfc3339()),
                source_url: Some(issue.html_url.clone()),
                confidence: 1.0,
                extraction_method: "Issue body".into(),
                explicit_fact: true,
                superseded_by: None,
                extracted_at: Utc::now(),
                observed_at: issue.updated_at,
            })
            .await
    }

    /// Ingest only explicit documentation and bounded source files. No build,
    /// install or generated command is executed while indexing.
    pub async fn ingest_local_documents(&self, repository_id: &str, root: &Path) -> Result<usize> {
        let root = tokio::fs::canonicalize(root)
            .await
            .map_err(|_| GitPilotError::Validation("repository path is unavailable".into()))?;
        let mut ingested = 0;
        for name in ["README.md", "README", "CONTRIBUTING.md", "ARCHITECTURE.md"] {
            let path = root.join(name);
            if !crate::local_git::path_is_inside(&root, &path) {
                continue;
            }
            if let Ok(content) = bounded_read(&path).await {
                self.store
                    .save_knowledge(&KnowledgeRecord {
                        id: Uuid::new_v4().to_string(),
                        repository_id: Some(repository_id.into()),
                        kind: "documentation".into(),
                        title: name.into(),
                        content,
                        source_object: name.into(),
                        source_version: Some("working-tree".into()),
                        source_url: None,
                        confidence: 0.95,
                        extraction_method: "Local documentation ingestion".into(),
                        explicit_fact: true,
                        superseded_by: None,
                        extracted_at: Utc::now(),
                        observed_at: Utc::now(),
                    })
                    .await?;
                ingested += 1;
            }
        }
        let docs = root.join("docs");
        let mut queue = vec![docs];
        let mut count = 0;
        while let Some(dir) = queue.pop() {
            if count >= 100 {
                break;
            }
            let Ok(mut entries) = tokio::fs::read_dir(&dir).await else {
                continue;
            };
            while let Ok(Some(entry)) = entries.next_entry().await {
                if count >= 100 {
                    break;
                }
                let file_type = match entry.file_type().await {
                    Ok(file_type) => file_type,
                    Err(_) => continue,
                };
                let path = entry.path();
                // Do not follow symlinks while indexing. A symlink inside a
                // repository must never expand the filesystem boundary.
                if file_type.is_symlink() || !crate::local_git::path_is_inside(&root, &path) {
                    continue;
                }
                if file_type.is_dir() {
                    queue.push(path);
                    continue;
                }
                let is_doc = matches!(
                    path.extension().and_then(|v| v.to_str()),
                    Some("md" | "mdx" | "adr")
                );
                if !is_doc {
                    continue;
                }
                if let Ok(content) = bounded_read(&path).await {
                    let relative = path
                        .strip_prefix(&root)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .into_owned();
                    self.store
                        .save_knowledge(&KnowledgeRecord {
                            id: Uuid::new_v4().to_string(),
                            repository_id: Some(repository_id.into()),
                            kind: "documentation".into(),
                            title: relative.clone(),
                            content,
                            source_object: relative,
                            source_version: Some("working-tree".into()),
                            source_url: None,
                            confidence: 0.9,
                            extraction_method: "Local documentation ingestion".into(),
                            explicit_fact: true,
                            superseded_by: None,
                            extracted_at: Utc::now(),
                            observed_at: Utc::now(),
                        })
                        .await?;
                    ingested += 1;
                    count += 1;
                }
            }
        }
        let mut code_queue = vec![root.clone()];
        let mut code_count = 0usize;
        while let Some(dir) = code_queue.pop() {
            if code_count >= MAX_CODE_FILES {
                break;
            }
            let Ok(mut entries) = tokio::fs::read_dir(&dir).await else {
                continue;
            };
            while let Ok(Some(entry)) = entries.next_entry().await {
                if code_count >= MAX_CODE_FILES {
                    break;
                }
                let file_type = match entry.file_type().await {
                    Ok(file_type) => file_type,
                    Err(_) => continue,
                };
                let path = entry.path();
                if file_type.is_symlink() || !crate::local_git::path_is_inside(&root, &path) {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
                if file_type.is_dir() {
                    if !matches!(
                        name.as_str(),
                        ".git"
                            | "target"
                            | "node_modules"
                            | "vendor"
                            | "generated"
                            | "dist"
                            | "build"
                            | ".next"
                            | "coverage"
                    ) {
                        code_queue.push(path);
                    }
                    continue;
                }
                let extension = path
                    .extension()
                    .and_then(|value| value.to_str())
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                if !matches!(
                    extension.as_str(),
                    "rs" | "ts"
                        | "tsx"
                        | "js"
                        | "jsx"
                        | "go"
                        | "py"
                        | "java"
                        | "kt"
                        | "rb"
                        | "php"
                        | "c"
                        | "h"
                        | "cc"
                        | "cpp"
                        | "hpp"
                        | "cs"
                        | "swift"
                        | "sql"
                        | "toml"
                        | "yaml"
                        | "yml"
                ) {
                    continue;
                }
                if let Ok(content) = bounded_read(&path).await {
                    let relative = path
                        .strip_prefix(&root)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .into_owned();
                    self.store
                        .save_knowledge(&KnowledgeRecord {
                            id: Uuid::new_v4().to_string(),
                            repository_id: Some(repository_id.into()),
                            kind: "code".into(),
                            title: relative.clone(),
                            content,
                            source_object: relative,
                            source_version: Some("working-tree".into()),
                            source_url: None,
                            confidence: 0.8,
                            extraction_method: "Bounded local source ingestion".into(),
                            explicit_fact: true,
                            superseded_by: None,
                            extracted_at: Utc::now(),
                            observed_at: Utc::now(),
                        })
                        .await?;
                    ingested += 1;
                    code_count += 1;
                }
            }
        }
        // Drive a repository-scoped code search index after bounded source
        // ingestion. This is still local-only and does not execute code.
        for symbol in [
            "async",
            "auth",
            "database",
            "error",
            "migration",
            "permission",
            "security",
            "test",
        ] {
            let hits = crate::local_git::LocalGit::search(
                &root.to_string_lossy(),
                symbol,
                &tokio_util::sync::CancellationToken::new(),
            )
            .await
            .unwrap_or_default();
            if hits.is_empty() {
                continue;
            }
            self.store
                .save_knowledge(&KnowledgeRecord {
                    id: Uuid::new_v4().to_string(),
                    repository_id: Some(repository_id.into()),
                    kind: "code_index".into(),
                    title: format!("Symbol index · {symbol}"),
                    content: hits.join("\n"),
                    source_object: format!("git-grep:{symbol}"),
                    source_version: Some("working-tree".into()),
                    source_url: None,
                    confidence: 0.9,
                    extraction_method: "Bounded Git grep index".into(),
                    explicit_fact: true,
                    superseded_by: None,
                    extracted_at: Utc::now(),
                    observed_at: Utc::now(),
                })
                .await?;
            ingested += 1;
        }
        Ok(ingested)
    }

    pub async fn retrieve(
        &self,
        query: &str,
        repository_id: Option<&str>,
    ) -> Result<Vec<RetrievalHit>> {
        let hits = self.store.search(query, repository_id, 30).await?;
        let knowledge = self.store.knowledge(repository_id, 500).await?;
        let mut results = Vec::new();
        for hit in hits {
            if let Some(record) = knowledge.iter().find(|r| r.id == hit.id) {
                results.push(RetrievalHit {
                    record: record.clone(),
                    score: 1.0,
                });
            }
        }
        if results.is_empty() {
            let needle = query.to_ascii_lowercase();
            for record in knowledge {
                if record.title.to_ascii_lowercase().contains(&needle)
                    || record.content.to_ascii_lowercase().contains(&needle)
                {
                    results.push(RetrievalHit { record, score: 0.5 });
                }
            }
        }
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(10);
        Ok(results)
    }

    pub async fn retrieve_hybrid(
        &self,
        query: &str,
        repository_id: Option<&str>,
        permissions: &PermissionPolicy,
        router: &RouterHandle,
        provider: Option<&ProviderConfig>,
        vector_weight: f64,
    ) -> Result<HybridRetrieval> {
        let mut lexical = self.retrieve(query, repository_id).await?;
        let mut missing = Vec::new();
        let Some(provider) = provider else {
            return Ok(HybridRetrieval {
                hits: lexical,
                vector_used: false,
                embeddings_indexed: 0,
                missing: vec!["No embedding provider is configured; using FTS5 only.".into()],
            });
        };
        let Some(model) = provider.embedding_model.as_deref() else {
            return Ok(HybridRetrieval {
                hits: lexical,
                vector_used: false,
                embeddings_indexed: 0,
                missing: vec![format!(
                    "{} does not have an embedding model configured; using FTS5 only.",
                    provider.display_name
                )],
            });
        };
        let indexed = self
            .store
            .knowledge_embeddings(repository_id, &provider.id, model)
            .await?;
        if indexed.is_empty() {
            return Ok(HybridRetrieval {
                hits: lexical,
                vector_used: false,
                embeddings_indexed: 0,
                missing: vec![
                    "No embeddings are indexed for the selected provider/model; run semantic indexing first."
                        .into(),
                ],
            });
        }
        let query_response = router
            .embed(
                &provider.id,
                model,
                &[query.chars().take(8_000).collect()],
                permissions,
                &tokio_util::sync::CancellationToken::new(),
            )
            .await?;
        let Some(query_vector) = query_response.vectors.first() else {
            missing.push("The embedding provider returned no query vector.".into());
            return Ok(HybridRetrieval {
                hits: lexical,
                vector_used: false,
                embeddings_indexed: indexed.len(),
                missing,
            });
        };
        let semantic = self
            .store
            .semantic_knowledge_search(query_vector, repository_id, &provider.id, model, 30)
            .await?;
        let mut combined =
            std::collections::BTreeMap::<String, (KnowledgeRecord, f64, bool)>::new();
        let vector_weight = vector_weight.clamp(0.0, 1.0);
        for (index, hit) in lexical.drain(..).enumerate() {
            combined.insert(
                hit.record.id.clone(),
                (hit.record, 1.0 - index as f64 * 0.01, false),
            );
        }
        for (record, score) in semantic {
            combined
                .entry(record.id.clone())
                .and_modify(|entry| {
                    entry.1 = (entry.1 * (1.0 - vector_weight) + score * vector_weight)
                        .max(entry.1 * 0.8 + score * vector_weight);
                    entry.2 = true;
                })
                .or_insert((record, score * 0.8, true));
        }
        let mut hits = combined
            .into_values()
            .map(|(record, score, semantic)| RetrievalHit {
                record,
                score: if semantic { score + 0.05 } else { score },
            })
            .collect::<Vec<_>>();
        hits.sort_by(|left, right| {
            right
                .score
                .partial_cmp(&left.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        hits.truncate(10);
        Ok(HybridRetrieval {
            hits,
            vector_used: true,
            embeddings_indexed: indexed.len(),
            missing,
        })
    }

    pub async fn index_embeddings(
        &self,
        repository_id: Option<&str>,
        permissions: &PermissionPolicy,
        router: &RouterHandle,
        provider: &ProviderConfig,
        max_records: usize,
    ) -> Result<usize> {
        let model = provider.embedding_model.as_deref().ok_or_else(|| {
            GitPilotError::Validation("provider has no embedding model configured".into())
        })?;
        let records = self
            .store
            .knowledge_for_embedding(repository_id, &provider.id, model, max_records)
            .await?;
        let mut indexed = 0;
        for chunk in records.chunks(16) {
            let inputs = chunk
                .iter()
                .map(|record| {
                    format!(
                        "TITLE: {}\nKIND: {}\n{}\n",
                        record.title,
                        record.kind,
                        record.content.chars().take(4_000).collect::<String>()
                    )
                })
                .collect::<Vec<_>>();
            let response = router
                .embed(
                    &provider.id,
                    model,
                    &inputs,
                    permissions,
                    &tokio_util::sync::CancellationToken::new(),
                )
                .await?;
            if response.vectors.len() != chunk.len() {
                return Err(GitPilotError::Provider(
                    "embedding provider returned a partial batch".into(),
                ));
            }
            for (record, vector) in chunk.iter().zip(response.vectors.iter()) {
                let hash = crate::store::knowledge_content_hash(&record.title, &record.content);
                self.store
                    .save_knowledge_embedding(&record.id, &provider.id, model, vector, &hash)
                    .await?;
                indexed += 1;
            }
        }
        Ok(indexed)
    }

    pub fn answer_from_evidence(
        query: &str,
        hits: &[RetrievalHit],
        repositories: &[Repository],
    ) -> crate::domain::AiExplanation {
        if hits.is_empty() {
            return crate::domain::AiExplanation {
                generated_at: Utc::now(),
                provider: "Local retrieval".into(),
                model: "FTS5".into(),
                one_sentence: format!("No indexed evidence was found for \"{query}\"."),
                executive: "This is not evidence that the answer is negative; the relevant source may not be indexed or accessible.".into(),
                developer: "Run repository synchronization or index local documentation, then ask again.".into(),
                technical: String::new(),
                review_focus: vec![],
                uncertainties: vec!["No matching knowledge record was retrieved.".into()],
                citations: vec![],
                file_notes: vec![],
                input_tokens: 0,
                output_tokens: 0,
                estimated_cost_usd: 0.0,
            };
        }
        let citations = hits
            .iter()
            .map(|hit| crate::domain::EvidenceRef {
                label: hit.record.title.clone(),
                url: hit.record.source_url.clone(),
                object_id: Some(hit.record.id.clone()),
                kind: if hit.record.explicit_fact {
                    crate::domain::EvidenceKind::Observed
                } else {
                    crate::domain::EvidenceKind::Inference
                },
            })
            .collect::<Vec<_>>();
        let summary = hits
            .iter()
            .take(3)
            .map(|hit| {
                format!(
                    "{}: {}",
                    hit.record.title,
                    hit.record.content.chars().take(280).collect::<String>()
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        let repo_scope = repositories
            .iter()
            .filter(|r| {
                hits.iter()
                    .any(|h| h.record.repository_id.as_deref() == Some(&r.id))
            })
            .map(|r| r.full_name.clone())
            .collect::<Vec<_>>()
            .join(", ");
        crate::domain::AiExplanation {
            generated_at: Utc::now(),
            provider: "Local retrieval".into(),
            model: "FTS5 + provenance".into(),
            one_sentence: format!(
                "{} relevant source record{} found{}.",
                hits.len(),
                if hits.len() == 1 { "" } else { "s" },
                if repo_scope.is_empty() {
                    String::new()
                } else {
                    format!(" across {repo_scope}")
                }
            ),
            executive: format!(
                "The answer is grounded in {}. Treat generated synthesis as inference and follow the cited source for authoritative detail.",
                hits.len()
            ),
            developer: summary,
            technical: "Retrieval uses repository-scoped SQLite FTS5 with provenance fields (source object, version, observation time, confidence and explicit-fact flag). Freshness is not inferred beyond the stored timestamps.".into(),
            review_focus: hits.iter().take(5).map(|h| h.record.title.clone()).collect(),
            uncertainties: vec![
                "Records may be stale if synchronization has not run.".into(),
                "No negative conclusion is possible when a source is unavailable.".into(),
            ],
            citations,
            file_notes: vec![],
            input_tokens: 0,
            output_tokens: 0,
            estimated_cost_usd: 0.0,
        }
    }

    pub async fn ingest_repository_metadata(&self, repo: &Repository) -> Result<()> {
        let content = format!(
            "{}\nDefault branch: {}\nPrimary language: {}\nVisibility: {}\nOrigin: {}",
            repo.description.as_deref().unwrap_or("No description"),
            repo.default_branch,
            repo.language.as_deref().unwrap_or("Unknown"),
            if repo.private { "private" } else { "public" },
            repo.origin.label()
        );
        self.store
            .save_knowledge(&KnowledgeRecord {
                id: Uuid::new_v4().to_string(),
                repository_id: Some(repo.id.clone()),
                kind: "repository".into(),
                title: repo.full_name.clone(),
                content,
                source_object: repo.full_name.clone(),
                source_version: Some(repo.updated_at.to_rfc3339()),
                source_url: Some(repo.html_url.clone()),
                confidence: 1.0,
                extraction_method: "GitHub repository metadata".into(),
                explicit_fact: true,
                superseded_by: None,
                extracted_at: Utc::now(),
                observed_at: repo.updated_at,
            })
            .await
    }
}

async fn bounded_read(path: &Path) -> Result<String> {
    let metadata = tokio::fs::metadata(path).await?;
    if metadata.len() > MAX_INGEST_BYTES {
        return Err(GitPilotError::InvalidInput(
            "document exceeds indexing size limit".into(),
        ));
    }
    let bytes = tokio::fs::read(path).await?;
    Ok(String::from_utf8_lossy(&bytes[..bytes.len().min(MAX_INGEST_BYTES as usize)]).into_owned())
}

pub fn provenance_label(record: &KnowledgeRecord) -> &'static str {
    if record.explicit_fact {
        "Explicit fact"
    } else {
        "Generated hypothesis"
    }
}

pub fn data_origin_for_record(record: &KnowledgeRecord) -> DataOrigin {
    if record.source_url.is_some() {
        DataOrigin::GitHub
    } else {
        DataOrigin::LocalGit
    }
}

pub fn bounded_paths(root: &Path, max: usize) -> Vec<PathBuf> {
    let mut output = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return output;
    };
    for entry in entries.flatten().take(max) {
        let path = entry.path();
        if path.is_file() {
            output.push(path);
        }
    }
    output
}

#[cfg(test)]
mod symlink_tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn local_ingestion_does_not_follow_symlinks() {
        let root = tempdir().unwrap();
        let docs = root.path().join("docs");
        tokio::fs::create_dir_all(&docs).await.unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(outside.path(), b"outside secret").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(outside.path(), docs.join("leak.md")).unwrap();
        tokio::fs::write(docs.join("safe.md"), b"inside documentation")
            .await
            .unwrap();
        let store = crate::store::Store::open_in_memory().await.unwrap();
        crate::demo::seed(&store).await.unwrap();
        let knowledge = KnowledgeService::new(store);
        let count = knowledge
            .ingest_local_documents("repo-0001", root.path())
            .await
            .unwrap();
        assert!(count >= 1);
        let records = knowledge
            .store
            .knowledge(Some("repo-0001"), 100)
            .await
            .unwrap();
        assert!(!records.iter().any(|r| r.content.contains("outside secret")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_retrieval_is_not_negative_evidence() {
        let answer = KnowledgeService::answer_from_evidence("why did auth change", &[], &[]);
        assert!(answer.executive.contains("not evidence"));
    }
}
