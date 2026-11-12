# Zapture contributor instructions

Keep the product simple: record, stop, screenshot, sound source, files. English UI. Local processing. No accounts, telemetry, ads or paid-service dependencies. Preserve upstream MIT attribution.

Windows code is in src/app.rs and src/win. Linux/macOS preview code is in src/desktop.rs. MCP protocol and tool allowlist are in src/mcp.rs. Never expose arbitrary shell execution or arbitrary file access through capture tools. stdout must remain protocol-only in MCP mode.

Run cargo test and compile the affected target. Cross-compilation does not prove recording works on hardware. Keep README platform support and ROADMAP truthful: Wayland capture, native macOS system audio and mobile are not implemented. Do not label placeholders as supported features.

Agent use/configuration: read MCP.md. Record with audio off unless requested. Poll status after stopping. Do not infer success from file existence or process startup alone.
