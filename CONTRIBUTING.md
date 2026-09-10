# Contributing

Thank you for improving Workshop. Open an issue before undertaking a large
change, keep pull requests focused, and explain user-visible behavior and
compatibility implications.

Run `./scripts/install.sh --check`, `./scripts/install.sh`, and
`./scripts/workshop.sh build-and-run` from a clean clone. Generated protocol
bindings must be regenerated whenever the Rust command surface changes.
Do not commit credentials, local absolute paths, generated build output,
test corpora, or release evidence. Maintainers run the private verification
overlay against the exact source candidate before publication.

By submitting a contribution, you certify the Developer Certificate of Origin
1.1 for that contribution and agree that it is licensed under Apache-2.0.
Include a `Signed-off-by` trailer in each commit (`git commit -s`).
