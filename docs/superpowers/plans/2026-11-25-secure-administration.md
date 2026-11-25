# Secure administration implementation plan

**Goal.** Deliver the 0.2 administration workflow. An operator claims the first account from the local CLI, signs in through the private listener, and creates, edits, tags, exports, and deletes short links.

**Architecture.** Store credentials, setup and recovery claims, sessions, and scoped API keys in SQLite. Use Argon2id for passwords, random tokens with stored digests, exact admin origin checks, and same-origin CSRF protection. Serve the static Svelte and TypeScript interface from the existing Rust binary. Keep the redirect listener independent.

1. Add a versioned authentication schema and service. Cover one-time and concurrent setup, expiry, login, revocation, recovery, and scope checks. Restrict the CLI to token provisioning and print secrets only to its output.
2. Add authenticated link update and deletion, enable and disable, and normalized tags. Keep existing host and slug identity stable. Test scoped access, multi-table atomicity, and preservation of stored redirect targets.
3. Replace the development bearer flow in the runtime with revocable sessions and scoped API keys. Use a canonical configured admin origin, secure cookies on HTTPS, strict cookie and header parsing, CSRF checks, and bounded rate and password hashing work. Validate these policies through both HTTP routers.
4. Build the Svelte admin interface with offline runtime assets, localized text, and dark, light, and system themes. Exercise the real setup and link workflow on a CI browser. Configure a build-only Node stage and keep the runtime image non-root.
5. Verify the Rust integration suite, schema, formatting, Clippy, frontend build, and a disposable Docker deployment. Update the English project documentation and publish through a tested GitHub pull request.
