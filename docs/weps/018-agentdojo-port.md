# WEP 018: The AgentDojo port

**Status:** Accepted 2026-09-25. Banking, Slack and workspace suites ported; travel and a real-model run remain.

## Decision

- **One Wardscript function per user task**, written from the request, in
  `benchmarks/agentdojo/<suite>/main.ward`. The request is the trusted plan; tools and
  models are untrusted. This is CaMeL's split, with the programmer as the planner.
- **AgentDojo's environments run as MCP servers**, so the port goes through the
  same typed imports, `ward.lock` and runtime sink checks as any program.
- **Security is measured against an adversarial model**: it lies in every answer, or
  answers honestly except at one point (one function, one record field, one extra list
  element), and an attack counts if any strategy reaches the goal. A blanket lie trips
  the first check, so on its own it overstates security; the single deviations found the
  Slack invitation that only the approver stops. It doesn't depend on how persuasive the attack text is
  or on which model is used, and a result of 0 is a claim about the program. Human
  approval is reported both ways (careful, and approving everything), so it's clear
  which results depend on the human.
- **Blind ports** (`<suite>/blind/`) are written by a fresh model session given only the
  requests, the tool schemas and the language docs, and graded by the same harness with
  honest answers written afterwards. They measure what the language guarantees for an
  author who doesn't know the attacks, not only for its designer.
- **Real models are opt-in** (`run.py --model`), outside CI, like the other live tests.
- **The naive versions are attack cases** (`tests/attacks/agentdojo_*`) and must
  fail with W0107. The evaluation also runs as an e2e test.

## Why

AgentDojo's own numbers measure a model's resistance to persuasion. The claim
Wardscript makes is different: the attacker can control every model answer and
still not reach a sink. An adversarial mock tests that claim directly and
deterministically in CI.

## Not done

- The travel suite.
- A run with a real model, to put utility next to AgentDojo's baselines (needs an API key).
