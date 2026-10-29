# ADR 0001: Native Rust and egui

Status: accepted

Use Rust stable and eframe/egui for all application logic and UI. A native
desktop binary avoids an embedded browser runtime, keeps the security boundary
visible and supports dense keyboard-first engineering workflows. The UI is split
into screens and reusable widgets with design tokens.

Trade-offs: egui requires custom work for rich text tables and accessibility;
these are addressed with core rendering primitives, keyboard navigation and
large hit targets. A web stack was rejected because the assignment requires a
Rust implementation and native credential/process integration.
