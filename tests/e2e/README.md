# End-to-end tests

Programs are built to Python with `ward build`, imported, and run against the
deterministic mock model. The harness arrives with M3.

Tests against real LLMs are opt-in with `WARD_LIVE=1`.
