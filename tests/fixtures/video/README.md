# Video test fixtures (Phase 6)

Two tiny synthetic clips so the AVFoundation path is exercised by `cargo test`
without ffmpeg and without touching any real user media. Both are read-only
fixtures: no test writes to them.

| File | Bytes | sha256 (prefix) | What it is |
|---|---|---|---|
| `tiny.mp4` | 2457 | `ecf779f7fe9970d1` | 64x48, 2.0 s, 10 fps H.264 video track; QuickTime creation date `2026-10-14T09:30:00+0900` |
| `audio-only.mp4` | 8580 | `8555f8dd51a361ba` | 44.1 kHz AAC audio only, no video track (2.0 s) |

Both are synthetic. No user media, no downloaded media, no third-party assets.

## How they were generated (development-time only, never at test time)

`tiny.mp4`: a hand-written Swift program linked against AVFoundation used
`AVAssetWriter` (H.264, 20 kbps) to encode a 64x48 gradient with a moving
square, plus a `com.apple.quicktime.creationdate` metadata item. (
The usual `ffmpeg` path was unavailable on the machine that produced this
fixture: the installed build fails to launch with a missing `libx265` dylib,
so Apple's own encoder was used instead.)

`audio-only.mp4`: a hand-written 16-bit PCM WAV (440 Hz sine, generated with
Python's standard library) converted by the macOS system tool `afconvert`:

```bash
afconvert -f mp4f -d aac -b 32000 sine.wav audio-only.mp4
```

## Why they are committed

The CI macOS runners have AVFoundation but no media tooling. A committed
fixture lets the integration tests exercise the real extraction path
(metadata values, frame count, audio-only refusal) on any macOS build of this
repository, while stub builds (no Swift toolchain or no Foundation Models SDK)
report an explicit `unavailable` and the same tests accept that documented
degradation.

## Rules

- Never replace these files with real recordings; keep them synthetic.
- Keep them small (a few kilobytes): they are repository payload, not samples.
- Tests must keep tolerances on measured values (see
  `tests/v095_ai_rename_p6_video_test.rs`); exact encoder output may differ
  after a regeneration.
