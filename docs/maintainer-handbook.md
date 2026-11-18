# Maintainer Handbook

This handbook is the operating guide for maintainers of this repository. It complements the contributor quickstart by covering code ownership, architecture boundaries, local testing, schema and contract changes, incident response, and release sign-off. Every section links to the automation that enforces or supports it.

If you are new to the project, read the contributor quickstart first, then return here before taking on review or release duties.

## 1. Architecture boundaries

The codebase is organized into layers with explicit boundaries. Keep changes within the layer that owns the behavior, and route cross-layer changes through the owning maintainers.

- **Core domain / library code** — business logic and shared utilities. Must not depend on CLI, transport, or presentation layers.
- **Adapters / integrations** — external services, storage, and transport. Depend on core, never the reverse.
- **CLI / entrypoints** — thin wrappers that parse input and call into core or adapters.
- **Schemas and contracts** — versioned interfaces shared across components. Treated as public API (see Section 4).

Boundary rules are enforced by the CI workflows in `.github/workflows/`. Review the workflow definitions for the authoritative list of checks that run on every pull request. When in doubt about ownership, check `CODEOWNERS` and request review from the listed owners rather than merging across boundaries unilaterally.

## 2. Local test commands

Run these from a clean checkout before opening a pull request. They mirror the checks run in CI.

```bash
# Install dependencies (use the project's documented package manager)
# See the contributor quickstart for the exact install command.

# Run the full test suite
# (replace with the project's canonical test command)
<test-command>

# Run linters / formatters
<lint-command>

# Run schema and contract validators (see Section 4)
<validate-command>
```

The canonical commands are defined in the CI workflow files under `.github/workflows/`. If a command here drifts from CI, treat CI as the source of truth and update this handbook in the same pull request.

Verify commands in a clean checkout (fresh clone, no cached build artifacts) before relying on them. If a command fails only in a clean checkout, that is a bug in the setup instructions — fix it here.

## 3. Code review and ownership

- Every pull request requires review from a code owner listed in `CODEOWNERS`.
- Changes touching architecture boundaries (Section 1) require review from the owning layer's maintainers.
- Changes touching schemas or contracts (Section 4) require review from the contract owners and a documented migration path.
- Do not self-merge changes to release automation, CI workflows, or this handbook without a second maintainer's approval.

## 4. Schema and contract change workflow

Schemas and contracts are treated as public API. Follow this workflow for any change:

1. **Propose** — open an issue describing the change, the affected consumers, and the compatibility impact (additive vs. breaking).
2. **Version** — additive changes may land in the current version; breaking changes require a new major version and a deprecation window.
3. **Migrate** — provide a migration guide and, where applicable, a compatibility shim.
4. **Validate** — run the schema/contract validators locally (Section 2) and confirm the corresponding CI check passes.
5. **Document** — update the schema reference docs and changelog in the same pull request.

The validators and compatibility checks run automatically in CI via the workflows under `.github/workflows/`. A schema change that fails validation must not be merged.

## 5. Incident contacts and response

When an incident affects production or released artifacts:

1. **Declare** — open an incident channel and page the on-call maintainer per the project's on-call rotation.
2. **Triage** — identify the affected component and owning layer (Section 1).
3. **Mitigate** — prefer rollback to the last known-good release over forward-fixing under pressure.
4. **Communicate** — post status updates in the incident channel at a regular cadence.
5. **Follow up** — file a post-incident review within the project's SLA and link it from the incident record.

Incident runbooks live alongside the operational documentation referenced from the repository's docs index. Keep contact and rotation details in the project's operational runbook rather than duplicating them here, so there is a single source of truth.

## 6. Release sign-off

Releases require explicit sign-off from at least one maintainer who did not author the release changes.

Sign-off checklist:

- [ ] All required CI checks pass on the release commit (see `.github/workflows/`).
- [ ] Schema and contract validators pass (Section 4).
- [ ] Changelog and release notes are updated.
- [ ] Version numbers are bumped per the project's versioning policy.
- [ ] Rollback plan is documented and the last known-good release is identified.
- [ ] A second maintainer has reviewed and approved the release.

Record sign-off in the release pull request or release ticket. Do not publish a release without a recorded approval.

## 7. Reference list

The following references are checked by the handbook link check in CI. Keep them valid when editing this document.

- Contributor quickstart: `docs/contributing.md`
- Code owners: `CODEOWNERS`
- CI workflows: `.github/workflows/`
- Schema and contract validators: referenced from the CI workflows in `.github/workflows/`
- Incident runbooks: referenced from the repository docs index
- Release notes / changelog: `CHANGELOG.md`

## 8. Keeping this handbook current

- Update this handbook in the same pull request as any change to the automation it references.
- The handbook link check runs in CI (see `.github/workflows/`) and fails on broken references in Section 7.
- Review this handbook at each major release and after any incident that reveals a gap.