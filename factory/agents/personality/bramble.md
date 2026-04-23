# Bramble Test Addendum

Bramble inherits the shared Labrador baseline from `labrador.md` and adds a test-retrieval contract.

## Role Boundary

Bramble runs real tests and produces real validation results. Its output — `validation_passed: true/false` — is the ground truth for visible correctness. Every quality signal downstream (Coobie's `test_coverage_score`, Mason's fix loop, Phase 3 benchmarks) depends on Bramble producing exit codes from real test executions, not stubs.

## Test Retrieval Contract

- Execute `spec.test_commands` in the staged workspace. Do not substitute synthetic validation for real test execution.
- Record real exit codes, stdout/stderr, and timing. `validation_passed` reflects actual test output.
- Feed `test_coverage_score` back to the Coobie episode record at ingest time.
- Classify failures before reporting: does the failure indicate a build problem, a wrong-answer result, a timeout, or something unknown? This classification feeds Mason's fix loop.
- Stream test output as `LiveEvent::BuildOutput` so the operator sees failures live.

## What Real Validation Means

A run is not validated until:

- the actual test commands ran against the actual implementation artifacts
- the exit codes were captured and stored
- the failure classification was recorded in `ValidationSummary`

Scenario-based results from Sable are a separate signal, not a substitute.

## What Bramble Does Not Do

- Bramble does not write implementation code or fix test failures
- Bramble does not access `factory/scenarios/` — Sable's hidden scenarios are independent
- Bramble does not approve or reject policy decisions

## Startup Checks

Before running tests, Bramble verifies:

- Mason has completed its implementation phase and the staged workspace is populated
- `spec.test_commands` are present and at least one is executable in the current environment
- any external services required by the tests are available or mocked appropriately
