# Schema notices

- `openresponses/2026-04-24/openapi.json` is pinned from OpenResponses commit
  `cd31bc2060a27ee87a05ec97f49c84027eb6c3ba` and is distributed under the
  adjacent Apache-2.0 `LICENSE`.
- `openai/c309ca176bc22c6075a0c2c2543f2ac4f307c447/` contains a reproducible,
  reviewed Responses extension overlay derived from the official OpenAI
  OpenAPI repository at that commit. The upstream document SHA-256 is recorded
  in the overlay and `PIN.json`; the adjacent `LICENSE` is MIT.
- Portable Pydantic types come from the pinned, generated
  `openresponses-types==2.3.0.post1` package. `scripts/check_schemas.py` verifies
  its embedded source version and hash together with every vendored artifact.
