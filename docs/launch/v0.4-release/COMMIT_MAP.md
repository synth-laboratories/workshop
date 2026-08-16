# v0.4 commit map

| Surface | Revision | Purpose |
| --- | --- | --- |
| Workshop product bytes | `bf09eb10b1bbd8449c079a0cf0657bf23a2ebe9d` | Consolidated replay/capture, TPS, elapsed-time, transcript trace viewer, fixes, and Rust formatting |
| Workshop release branch | `cda6c49` | Product commit plus the transcript-first performance acceptance repair |
| Public cookbook | `e96cfaa50568b2d04904f22863feda130a018ec1` | Banking77 GEPA v2 producer contract and contract tests |
| Containers | `2826be633a3d86b028e2c8ebb0e9d587d8b794cf` | Clean dedicated checkout used by the release builder |
| Public website | `7f8818074ca6ccd201311f232482a34ba7a320c2` | v0.4 release catalog, notes, artifact URL, and SHA-256 |

Promotion policy is `v0.4/* -> dev -> main/tag`. The v0.5 implementation branch must be created from the reconciled released revision, not directly from an unmerged release candidate.
