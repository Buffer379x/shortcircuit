# ShortCircuit contributor guidance

- Read the [project status](docs/PROJECT_STATUS.md) and [decisions](docs/DECISIONS.md) before changing the project. The current milestone is 0.5 backup and data exchange. Keep project documentation in English.
- Use Codex for the initial implementation. Develop ShortCircuit independently and document its own behavior and contracts.
- Use functional copy throughout the UI, including sign-in. Maintain English and German UI strings centrally. Follow the [design system](docs/DESIGN_SYSTEM.md).
- Run one modular Rust process in one unprivileged Docker container. Keep the public and admin listeners separate and SQLite on persistent local storage. Activate outgoing integrations explicitly.
- Recreate previous short URLs manually when needed. Verify the public host, exact slug, and expected redirect before updating production routing.
- Keep visit limits transactional. Bound analytics and background work separately. Give security, data integrity, and reliability priority over performance improvements.
- Keep changes and commits focused. Version the schema and API. Run the relevant [tests](docs/TESTING.md) and report actual results. Use synthetic local test data and keep credentials, tokens, and personal data out of source, logs, and artifacts.
- Update the [threat model](docs/THREAT_MODEL.md) and [security guide](docs/SECURITY.md) when trust boundaries change. Add regression coverage for changes to host routing, auth, outgoing requests, restore, and import.
- Develop source in the public GitHub repository. Release artifacts, paid resources, license changes, and production infrastructure changes each require explicit approval.
