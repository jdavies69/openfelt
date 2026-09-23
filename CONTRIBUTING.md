# Contributing to OpenFelt

Play the hand. Learn the game.

New contributions are licensed under AGPL-3.0-or-later, the license of the combined application. By submitting a contribution, you confirm you have the right to contribute it under those terms. Preserve upstream copyright notices, including the original MIT notice in LICENSE-MIT, and identify third-party material and its license. See THIRD_PARTY_NOTICES.md for corresponding-source distribution requirements.

Use a topic branch. Run formatting, Clippy with warnings denied, and the locked all-target/all-feature test suite. New trainer behavior needs regression tests for accepted actions, coaching pauses, private information boundaries and chip accounting. CI and local play must not require an API key or paid model request.

Do not commit API keys, personal configurations, private learning histories or unredacted logs. Keep numerical poker claims tied to tested computations and stated assumptions. Label heuristic policies honestly; do not claim solver-optimal play without independent evidence.
