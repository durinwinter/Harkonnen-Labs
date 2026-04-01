---
tags: [workflow, scenario, oracle, parity, harkonnen, coobie]
summary: Reference-oracle regression pattern for Harkonnen runs over external product codebases.
---
When a run is really a scenario over an external product, Harkonnen should stay the factory and treat product components explicitly.

Pattern:
- code under test = product-owned workspace Harkonnen may change
- reference oracle = read-only known-good implementation or executable
- dataset = read-only preserved evidence bundle
- runtime API = optional product-owned live surface Ash may read
- evidence artifact = generated report Sable can judge

Behavior:
- Coobie retrieves project memory topics before Scout and Mason plan.
- Mason edits only the code-under-test component unless the spec expands scope.
- Bramble runs both visible product validation and the oracle-comparison harness.
- Sable judges evidence artifacts, not narrative optimism.
- Flint packages oracle inputs, outputs, and Coobie summaries together.

LamDet/Ceres example:
- Ceres Station Rust pipeline = code under test
- LamDet Python bundle = reference oracle
- CSV corpus = dataset
- parity report = evidence artifact
