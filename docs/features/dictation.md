# Dictation

Hold the configured shortcut, speak, and release. HEX transcribes locally and
inserts the result at the current focus. New captures do not cancel accepted
work; cancellation prevents insertion only before output commits.

This map covers the macOS release path. Linux shares the basic journey, not
every timing, input, or recovery guarantee; see the
[platform map](README.md#other-platforms-and-consumers). This is a map, not a
verified end-to-end driver. Linked checks are locators; no tests or previews
were run for this rewrite.

## How To Get To It

Settings shortcut capture persists the candidate before showing Saved. If the
write fails, capture remains active with an error; retry or cancel it explicitly.
`failed_shortcut_save_stays_in_capture_and_preserves_pending_edits` in
[app_window.rs](../../src/app_window.rs) checks this with injected persistence
failure, not a physical shortcut or installed-app disk failure.

**Shortcut:** Option by default, after macOS permissions and the selected local
model are ready. Commands and OpenCode are not required. Settings > Dictation
shortcut accepts modifier-only, modifier-plus-key, standalone Fn/Globe, and
standalone function-key bindings. Capturing a replacement suspends global matching.

**CLI:** `hex listen` uses saved settings; `--device` is authoritative. This is
the CLI listener, not Settings. Do not run it beside an existing listener.

**Separate targets:** [Voice Action](voice-action.md) has its own opt-in and
capture target, with no double-tap lock. Host-owned API capture has its own
ownership protocol.
Neither proves the physical dictation shortcut; see the [feature index](README.md).

## Sub-features

### Hold And Short Taps

```ts
Ordinary hold                            // lock activation is mapped below
  ├── Hold -> Speak -> Release -> Finish          // dictate.hold
  ├── Release before 300 ms -> Discard            // dictate.short-tap
  └── Early unrelated shortcut/click -> Discard   // not intentional dictation

Ordinary shortcut -> No recording mute/pause or idle-sleep prevention
Intentional recording -> No automatic duration limit
```

Source: `handle_hotkey_action` in [recognition.rs](../../src/recognition.rs).
Checks in [suppression.rs](../../src/suppression.rs):
`callback_timestamps_preserve_nanoseconds_and_short_tap_discard`,
`callback_press_reaches_intentional_hold_on_the_audio_clock`.
These cover constructed callback/reducer timing, not physical event delivery.

The short-tap threshold belongs to ordinary hold activation. Voice-delimited
and explicit programmatic capture have different intentional-start rules.

### Lock, With Or Without Hold

```ts
Settings > Double-tap to lock                         // dictate.lock
  Hold -> Ordinary hold path remains available
  Tap -> Tap
    ├── Second press AND release <300 ms after first release -> LOCKED
    └── Slow second release -> No lock

Settings > Double-tap only                            // dictate.double-tap-only
  Requires a key binding AND locking enabled
  Single press/hold -> No capture                     // disables hold, not a toggle
  Same two qualifying complete taps -> LOCKED         // same lock, not a third mode

LOCKED -> Speak hands-free -> Fresh shortcut press -> Finish
       -> Escape -> Cancel
       -> Missed key release -> Neutral keyboard repairs tracking -> Still LOCKED
```

Checks in [suppression.rs](../../src/suppression.rs):
`callback_double_tap_locks_for_modifier_and_key_bindings`,
`a_slow_second_release_does_not_lock`,
`double_tap_only_waits_for_two_complete_key_chord_taps`,
`locked_dictation_finishes_after_a_missing_key_release`.
These cover gesture decisions, not hardware keyboard-layout or remapping equivalence.

### Warm And Cold Capture

```ts
Keep ready -> Press shortcut                          // dictate.boundaries
  -> Include up to 450 ms of available pre-roll
  -> Delayed handling still uses original event boundaries
  -> Release excludes audio captured after physical release + 200 ms

Release or locked finishing press                     // dictate.release-tail
  -> Hold threshold judged on the physical release alone
  -> Wait for audio through release + 200 ms (POST_RELEASE_TAIL)
     ├── Arrives -> Clip ends exactly at release + 200 ms
     └── Stream stalls/fails -> Finish within tail + 100 ms with what arrived
  -> Same for Voice Action and promotion; not Escape, short taps, voice
     controls, programmatic stop, meetings, or Linux
  -> A press inside the tail starts after the old clip finishes, with its own pre-roll

Release when idle -> Press shortcut                   // dictate.cold-microphone
  -> Open microphone asynchronously, no pre-roll
     ├── Release before readiness -> Discard
     ├── Stream fails before the hold threshold -> Reopen for the same press
     └── Ready while held or locked -> Capture
  -> Capture idle -> Microphone can close             // accepted jobs do not keep it open
```

Source: `DictationCapture` in [dictation.rs](../../src/dictation.rs); checks:
`delayed_press_reconstructs_audio_from_the_original_boundary`,
`delayed_release_removes_audio_captured_after_the_physical_release`,
`capture_continues_past_sixty_seconds_until_explicitly_finished`,
`release_tail_keeps_audio_through_the_tail_and_no_further`,
`short_tap_discards_on_its_physical_release_despite_the_tail`,
`resampled_clips_keep_the_last_word_through_a_delayed_release` (44.1/48 kHz,
segmented and not, with the tail). These cover sample-buffer/timestamp logic, not
a microphone recording. The physical release stays exact; the 200 ms tail is an
explicit extension of the trim boundary. The opt-in `release_boundary_harness`
([dictation_tail_tests.rs](../../src/dictation_tail_tests.rs); `HEX_TAIL_MODEL`,
`HEX_TAIL_WAVS`) and `evaluate_dictation_corpus`
([dictation_corpus_tests.rs](../../src/dictation_corpus_tests.rs);
[`scripts/dictation-corpus.sh`](../../scripts/dictation-corpus.sh),
`HEX_DICTATION_CORPUS_DIR`) replay recordings through the capture timeline at
release offsets (`-300 ms` to `+200 ms`) and onset offsets (`-300 ms` to
`+100 ms`). On 2026-10-15, before the tail, across 102 clips (70 real voice
recordings, 20 macOS `say` clips, and 12 ElevenLabs `eleven_v4` clips) with
Unified English, clips matched the release to the sample; releasing at
`0 ms`..`+200 ms` kept the final word on 10/10 real clips (`-50 ms`: 9/10;
`-100 ms`: 7/10; `-150 ms`: 4/10; `-200 ms`: 2/10). Warm 450 ms pre-roll kept the
initial word even when speech began `-300 ms` before press, whereas cold 0 ms
pre-roll lost initial syllables when speech preceded readiness by `150–300 ms`.
Incremental on/off and 1 versus default decoder threads gave identical tail
results. The harnesses now apply the tail; they have not been re-run with it. The
tail also keeps any key-up or click sound made at release; no filtering is applied.
The owner in [dictation_audio.rs](../../src/dictation_audio.rs) has
`cold_capture_restart_rejects_late_open_results_and_closes_after_finish` and
`pending_open_uses_physical_press_for_intentional_threshold`: controlled opening
and ownership transitions, not physical device startup latency. Tail checks there
drive the owner with a fake stream: `release_waits_for_its_tail_and_keeps_no_later_audio`,
`a_press_inside_the_tail_starts_its_own_capture_after_the_old_one_finishes`,
`a_stalled_or_failed_stream_bounds_the_tail_wait`, and
`escape_and_short_taps_finish_without_waiting_for_a_tail`. The coordinator blocks
on the tail, so the stop tone, HUD Transcribing, and submission follow the release
by up to 200 ms (300 ms on a stalled stream); their order is unchanged.

### Optional Voice Controls

Optional spoken entry requires Commands enabled, its model ready, and recognition
awake. [Voice Commands](commands.md#spoken-dictation-controls) owns
`dictate.voice-controls`, its start/stop/Send/cancel phrases, config, and checks;
this page owns the resulting capture and output.

### Cloud Transcription (Opt-in)

`cloud_transcription.enabled` (default off) sends each finished dictation clip to
a hosted speech-to-text API instead of the local model. Setup and keys live in
[cloud-transcription.md](../cloud-transcription.md); there is no Settings UI yet.

```ts
Release with cloud enabled                 // dictate.cloud
├── macOS Paste / Send / Voice Action -> whole clip, no incremental pieces
│   └── local API (Service target), meetings -> stay on the local model
├── Linux service, `hex dictate`, Windows `dictate` -> whole clip
├── 16 kHz mono PCM16 WAV -> multipart POST -> JSON `text`
│   ├── OpenAI   -> /v1/audio/transcriptions (gpt-4o-mini-transcribe)
│   ├── xAI      -> /v1/stt (provider default model)
│   └── Groq     -> /openai/v1/audio/transcriptions (whisper-large-v3-turbo)
├── Clip > 10 min -> sequential uploads, joined with spaces
└── HTTP error, timeout, missing key -> job fails, nothing pastes
```

Key: `cloud_transcription.api_key`, else `OPENAI_API_KEY` / `XAI_API_KEY` /
`GROQ_API_KEY` in the transcribing process's environment. Endpoints require
HTTPS except loopback gateways. Linux skips the installed-model gate when cloud
is enabled; macOS onboarding still requires a local model. Checks:
`cloud_transcription::tests` (settings, WAV, multipart order, error parsing,
loopback round trip). No request against a real provider has been recorded.

### Ongoing Jobs And Output

```ts
Completed long recording                  // dictate.model-windows
├── Cohere -> Runs of at most 35 seconds, including the final remainder
└── Other GGUF models -> Existing backend-advertised input limit
    -> Transcribe each chunk -> Join text in original order

Chunking bounds inference, not capture duration. No silence is removed.
```

The shared `max_audio_chunk_samples` policy in
[transcription_models.rs](../../src/transcription_models.rs) is used by
[parakeet.rs](../../src/parakeet.rs) and
[linux_transcriber.rs](../../src/linux_transcriber.rs). Cohere's pinned backend
reports a 400-second encoder capacity, while its reference processor uses a
35-second transcription window. Treating the encoder capacity as a useful
inference window caused [#73](https://github.com/anomalyco/hex/issues/73)'s lost
passages. A stricter backend bound still wins; other model policies are unchanged.

`cohere_chunks_at_reference_window_not_encoder_capacity` checks the policy.
The opt-in `cohere_long_form_keeps_all_sections_with_and_without_pauses` runs the
real pinned model on synthetic speech. Reproduce it on macOS with
[`scripts/test-cohere-long-form.sh`](../../scripts/test-cohere-long-form.sh) and an
explicit model path. The script uses Samantha speech synthesis, never microphone
capture, and retains fixtures on failure (`HEX_KEEP_COHERE_FIXTURES=1` keeps successes).

**Observed September 4, 2026:** the same 57-second fixture went from 44 word errors
to zero after chunking; the 73-second paused fixture went from 71 errors to one.
Short references transcribed correctly before and after. The retained native
regression also passed a roughly 235-second repeated fixture, retaining all three
sections in each repetition. This is Metal inference evidence on an M2 Max, not
physical capture/paste or Linux runtime proof. Chunk boundaries can still affect
individual words; this is not a claim of perfect recognition or silence handling.

The macOS `Transcriber` owns backend input padding for ordinary, voice-protocol,
and segmented transcription. `short_unified_input_is_minimum_padded_before_trailing_context`
in [transcription.rs](../../src/transcription.rs) checks minimum-duration padding
followed by Unified English's additional trailing silence. This is a pure sample
policy check, not native inference-quality proof. Padding is now included in
`inference_ms`; `prepare_ms` covers clip conversion and diagnostic retention.

```ts
Settings > Transcribe during pauses = On (default)  // dictate.incremental
Intentional hotkey capture (Dictation or Voice Action shortcut)
  -> Pause >= 600 ms after a piece >= 3 s, or >= 300 ms after >= 8 s
     // speech = 20 ms frame >= 12 dB above an adaptive floor, never below -55 dBFS
     -> Cut mid-pause -> Offer the piece to the inference worker, never blocking capture
        ├── Worker idle -> Transcribe the piece in the background; nothing pastes
        └── A piece already queued or running -> Keep the audio for the next piece
Release -> One accepted job for the whole clip
  ├── Every kept cut has a piece transcript, same model, not voice-delimited
  │    ├── Tail near-silent (< -50 dBFS) -> Skip tail inference
  │    ├── Tail < 3 s (the segmenter's minimum piece) -> Transcribe the last piece + tail
  │    │    together, replacing that piece's text   // a lone word after a pause has no context
  │    └── Otherwise -> Transcribe only the tail
  │    -> Join pieces + tail
  │       // after , ; : - or a mid-clause function word/hesitation/repeated word,
  │       // lowercase the next word (not I/acronyms) and drop isolated mid-clause periods;
  │       // otherwise close an unpunctuated piece before a capital
  └── Missing/failed/stale piece, model change, or voice-delimited capture
       -> Transcribe the full clip exactly as with the setting Off
  -> Mode processing, paste, last dictation, history once on the final text
Short tap, Escape, or new capture -> Piece transcripts are never used
Delayed release -> Cuts after release + 200 ms are dropped with their pieces
              // the final tail piece includes the 200 ms tail
Programmatic/voice-delimited captures and Linux -> Never segmented
```

Source: [pause_segmenter.rs](../../src/pause_segmenter.rs), `offer_segment` in
[dictation.rs](../../src/dictation.rs), the audio owner in
[dictation_audio.rs](../../src/dictation_audio.rs), and `SegmentSink`,
`SpeculativeTranscript`, `transcribe_clip`, and `join_segments` in
[parakeet.rs](../../src/parakeet.rs). The queue keeps two slots for final jobs;
at most one piece is admitted at a time. Logs record `joined incremental
transcription` with `pieces`/`tail_ms`/`merged_short_tail`, or `incremental
transcription fell back to the full clip` with a `reason`. Checks: the `pause_segmenter` tests (cut placement,
minimum length, long-piece pauses, noise floor, digital silence);
`segmented_pieces_match_the_final_clip_across_resampler_delays`,
`delayed_release_drops_cuts_after_the_physical_release`,
`rejected_pieces_merge_into_the_next_piece`, and
`short_taps_and_cancelled_captures_never_offer_pieces` in dictation.rs;
`hotkey_captures_hand_pieces_off_without_blocking_and_others_do_not` in
dictation_audio.rs; and `final_job_transcribes_only_the_tail_after_complete_pieces`,
`a_short_final_piece_is_transcribed_with_the_piece_before_it`,
`incomplete_stale_or_mismatched_pieces_fall_back_to_the_full_clip`,
`a_new_capture_discards_pieces_from_a_cancelled_capture`,
`piece_admission_never_takes_a_final_job_slot`, and
`join_segments_continues_clauses_and_closes_sentences` in parakeet.rs. These use
synthetic audio and fake transcription; they do not drive the threads end to end.

**Benchmark-only, October 14, 2026, Mac Studio, Unified English Q8, Metal:** the
opt-in `incremental_join_tracks_offline_transcription` feeds 16 kHz recordings
through the production capture, segmenter, plan, and join with the real model,
sequentially rather than in real time. On 24/77/99-second recordings it cut 3/7/6
pieces; release-time tail inference took 69/119/133 ms versus 208/715/1,270 ms for
the full clip, with 0/2/0 word differences from the full-clip transcript (66/235/280
words). An earlier real-time prototype measured 46–220 ms versus 0.7–3.3 s on
77–99-second clips. Neither is physical capture or paste on a MacBook Pro, and a
piece boundary can still change a word or its punctuation.

**Short final piece, October 15, 2026, Mac Studio, Unified English Q8, Metal:** a
retained 25-second dictation (private, not in the corpus) where the speaker paused
before the final word "world" was cut into 5 pieces plus a 989 ms tail. Transcribed
alone, the tail returned nothing (or "What" at release `-150 ms`), so every
incremental `release_boundary_harness` row ended "…an amazing"; with the merge,
every row from `-150 ms` to `+150 ms` ends "…an amazing world", matching the full
clip. Release inference went from 27–29 ms (tail only) to 37–51 ms (last piece +
tail) versus 102–177 ms for the full clip; the cost grows with the last piece's
length. [`scripts/dictation-pause-fixtures.sh`](../../scripts/dictation-pause-fixtures.sh)
writes four `say` clips with 900 ms of silence before one final word
(`tail_pause_word`) for both harnesses. Without the merge they kept the word but
split it into its own sentence ("an amazing. World", "finally. Done.") or lost
its capital ("by friday"); with it they match the full clip at every offset. TTS
does not reproduce the dropped word itself.

```ts
Listening idle                            // dictate.model-memory
├── Keep the selected model weights warm // avoids loading on each dictation
└── Completed offline inference
    -> Release input-sized GGML/Metal compute scratch
    -> Keep only reusable model/session state
```

The selected GGUF remains resident by design; Parakeet v3's pinned Q8 artifact is
739,508,576 bytes before runtime metadata and decoder state. Input-sized scheduler
scratch must not remain at its high-water mark after a dictation. The pinned
`transcribe-cpp` 0.1.3 runtime owns that scratch for the session lifetime, so HEX
rotates the lightweight session after every offline run while retaining the loaded
model. A later dictation recreates its session without reloading the weights.

**Observed September 18, 2026, isolated Metal probe on M2 Max:** with the similarly
sized Parakeet Unified English model, `transcribe-cpp` 0.1.3 grew from a 792 MB loaded
footprint to 1,195 MB after a 45-second input and retained 1,195 MB after another
short run. The same 0.1.3 runtime with HEX's session rotation measured 792 MB loaded,
801 MB after the long input, and 802 MB after the following short run. The probe used
generated silence and no live microphone, UI, Commands model, or paste. This
establishes the backend scratch cause and reclamation on that model and machine, not
Parakeet v3's exact whole-app footprint on the reporter's Mac. Activity Monitor can
include additional app and optional Commands-model memory.

The ignored `dictation_protocol_audio` native fixture was also run with the selected
Unified English model. Before and after session rotation it reached the same existing
second-pass mismatch (`say stop` decoded as `say stay`) on the question fixture, so
that run is not claimed as a passing transcription regression check.

```ts
Accepted job on a GPU device             // dictate.inference-threads
├── Inference thread -> macOS user-interactive QoS
└── GGUF session -> One CPU decoder thread, no ggml worker pool
    // CPU-only devices keep the backend's default thread count
```

Source: `session_options` in [gguf_session.rs](../../src/gguf_session.rs) and
`prioritize_inference_thread` in [parakeet.rs](../../src/parakeet.rs); check
`gpu_sessions_bound_decoder_threads_and_cpu_sessions_keep_library_default`.
Parakeet's per-token TDT decoder runs on CPU even with Metal encoding. ggml's
default pool (up to 8 spin-waiting threads) is created with plain `pthread_create`,
so its workers start at default QoS rather than inheriting the inference thread's.

**Observed October 14, 2026, standalone `transcribe-cpp` 0.2.4 bench on a Mac
Studio (18 cores) already busy with other work, Unified English Q8, Metal:** under
36 extra `yes` processes, user-interactive QoS medians for 3/30/90-second clips were
170/703/1,819 ms with one thread, 136/1,907/4,940 ms with two, and 178/4,304/13,519 ms
with four. Without the extra load, one thread measured 83/364/1,631 ms and two
64/705/2,889 ms. This is backend timing, not physical capture/paste on the
reporter's MacBook Pro, and not Linux runtime proof.

```ts
Finish
  ├── Near-silent capture -> Discard, no inference    // dictate.silence-gate
  │       // loudest 30 ms window (DC removed) below -50 dBFS; no paste,
  │       // History, or last-result update; process.log records the level
  ├── Capacity available -> Accepted job -> Local transcription -> Mode processing
  │    -> Paste successful nonempty output once, at current focus
  │       // the clipboard write carries nspasteboard.org Transient and
  │       // AutoGenerated markers, so conforming clipboard managers skip it
  │       // synthetic ⌘V resolves on any active layout: it uses the layout's
  │       // Command layer ("Dvorak – QWERTY ⌘"), and a non-Latin layout
  │       // falls back to the ASCII-capable layout like AppKit itself
  └── Queue full -> Explicit rejection                // bounded admission

A pending -> Start capture B                          // dictate.overlap
  -> B records independently; A is not cancelled
  -> Accepted outputs retain submission order
  -> Completing/pasting A must not end B
```

Source: `finish_dictation` in [recognition.rs](../../src/recognition.rs).
The silence gate stops models from hallucinating closing phrases such as "thank
you" on an empty hold ([#108](https://github.com/anomalyco/hex/issues/108)). It
covers hotkey, voice-delimited, and Voice Action captures on macOS, and the Linux
hotkey loop. The local API's programmatic dictation and host-audio transcription
are deliberately left ungated: they return the model's raw output to the caller.
The threshold is conservative, so speech is not dropped; a noisy room, the start
tone, or a key click can lift a capture above it, and those captures still reach
the model. Hallucinations after real speech are not addressed.
`silence_gate_discards_only_near_silent_captures` in
[dictation.rs](../../src/dictation.rs) checks the level policy on synthetic
samples, not real microphone noise floors.
Key resolution: [keyboard.rs](../../src/keyboard.rs); non-Latin layout
fallback details live in [shortcut recovery](recovery.md#keyboard-layout-resolution).
Checks in [parakeet.rs](../../src/parakeet.rs):
`parallel_processing_results_are_released_in_submission_order`,
`ordered_waiting_outputs_remain_bounded_after_channel_drain`.
These cover output bookkeeping/admission, not sustained end-to-end inference/paste.
`transcript_write_is_marked_for_clipboard_managers` in [paste.rs](../../src/paste.rs)
checks the markers on a private pasteboard; whether a given clipboard manager
(Maccy, Raycast) honors them is not verified here. The restored previous clipboard
is written back unmarked, exactly as captured.

### Cancellation Before Commit

```ts
Escape                                               // dictate.cancel
  ├── Active capture -> Cancel that capture first
  └── No active capture -> Cancel newest cancellable accepted job
     -> Repeat Escape -> Walk back through unfinished jobs

Cancelled before output commit -> No insertion
Already committed clipboard insertion -> Not undone   // not target-app undo
```

Checks in [parakeet.rs](../../src/parakeet.rs):
`output_stays_cancellable_through_preparation_but_not_after_mutation`,
`repeated_cancellation_walks_back_through_pending_jobs`.
These use a controlled paste boundary and temporary history store;
no foreground application consumes the clipboard.

### Feedback

```ts
Record -> Release -> Transcribing/processing/paste -> Finished // dictate.feedback
  HUD + tones distinguish capture from pending work          // never take focus
  Pending processing/paste -> Still unfinished, not recording
  Tone requested -> Playback worker opens the default output, then plays
     -> Output stays open five minutes past the last tone, then releases
     -> Released output holds no macOS idle-sleep assertion
  Feedback volume = 0 -> No tones
     -> Playback worker releases output on its next 250 ms observation
     -> Volume enabled again -> Reopen for the next tone, never capture/UI
  Cold audio stack exceeds preload admission -> Warn once; recognition continues
     -> Loader keeps running; later tones play once the output opens
```

Feedback admission is advisory on every platform: a cold audio stack can exceed
the two-second preload timeout, so the recognition worker logs a warning and
continues rather than stopping. The loader thread keeps running and registers
the player when the default output finally opens, restoring tones for the rest
of the session. Linux already logged and continued; the macOS worker now
matches that behavior instead of stopping desktop recognition.

The playback worker never opens the output at startup. It opens the default
output for the first tone, keeps it through a five-minute idle grace so bursts
of dictation share a warm device, and releases it after that grace or on the
next observation after sounds turn Off. macOS idle-sleep timers count from the
last user input, and a tone implies recent input, so the grace never delays
sleep by itself. A device open already in progress must return before the
worker can observe that change. Output-open failures retry at most once per
two seconds, and a released device reopens without that backoff. Bundled
samples remain decoded in memory. A tone that arrives after the grace pays a
cold device open first; the tone is delayed, not clipped or dropped.
Overlapping tones extend the playing window rather than shortening it.

**Fixed in 2.1.21:** through 2.1.20 the worker opened the output at startup
and kept it open for as long as sounds were On, so `coreaudiod` held a
`PreventUserIdleSystemSleep` assertion for HEX's PID and an idle Mac never
slept ([#95](https://github.com/anomalyco/hex/issues/95)). 2.1.18 released the
stream only when sounds were Off. Startup admission therefore no longer
reports `could not open the audio output`; a device failure surfaces as a
warning when the first tone plays.
`output_opens_for_a_tone_and_releases_after_the_idle_grace` and
`overlapping_tones_extend_playback_instead_of_truncating_it` check ownership,
grace, and drop with a controlled sink;
`failed_output_initialization_retries_with_a_bounded_backoff` checks
failure-to-success recovery without an audio device. The opt-in
`native_output_release_clears_the_idle_sleep_assertion` opens the real default
output and reads `pmset -g assertions`; on September 22, 2026 it observed the
assertion appear for the test PID and clear within a second of release on the
built-in speakers. That is not an installed-app or USB/Bluetooth measurement.
`timed_out_admission_still_publishes_a_usable_player` checks the production
publication path after the admission receiver has gone away. These checks do
not measure the reporter's CoreAudio CPU usage or establish audible playback.

**Contributor-reported September 10, 2026 (PR #84):** a locally built `/Applications/Hex.app`
(approximating 2.1.16 plus this change) was started right after a restart where
the stock build had stopped its desktop recognition worker with
`timed out preloading feedback audio`. The patched build started on the first
attempt: the worker reached `dictation listener started` and
`resolved paste key … paste_key_code=9`, and held-Option dictations pasted into
the focused app. The late-registration path (loader still running when the
admission wait times out) is implemented, but its end-to-end tone recovery has
not yet been observed on hardware;
it is expected behavior, not executed evidence.

Linux now uses the same recording sounds with its own persisted Sound volume
control; see [Linux feedback](README.md#other-platforms-and-consumers) for its
entry points, timing, defaults, and verification limits.

Check: [capture-preview.sh](../../scripts/capture-preview.sh), `dictation-hud`
preview. It repeats the recording/processing sequence until the preview exits,
so fullscreen and Space transitions can be checked without a ten-second deadline.
This renders a real window with deterministic state; it does not prove that
actual capture caused that state.

The macOS HUD is a click-through, nonactivating `NSPanel` with
`CanJoinAllSpaces`, `CanJoinAllApplications`, and `FullScreenAuxiliary` collection
behaviors. The cross-application flag explicitly allows the overlay to join
another application's fullscreen Space (#75); panel style flags on an ordinary
`NSWindow` were not sufficient.

**Observed September 14, 2026:** the unmodified HUD was not found on screen over
a native fullscreen TextEdit fixture. Changing only the window class to
`NSPanel` did not produce a visible HUD in the initial check. The candidate with
`CanJoinAllApplications` and the repeating fixture visibly showed the red HUD
over fullscreen TextEdit, captured through Cua Driver. This was a debug preview
with no microphone capture, not a signed installed-app or multi-display test.
Kit independently confirmed the repeating candidate was visible in fullscreen.

**Combined verification September 14, 2026:** 468 Rust tests passed with ten
native/opt-in checks ignored, plus all twelve keyboard-layout child scenarios
in debug and release. Strict all-target/all-feature Clippy, 46 command-SDK tests,
and the command-SDK build passed. The native preview also built successfully.
These checks preceded the app release.

**2.1.18 candidate verification September 14, 2026:** the optimized suite passed
all 468 Rust tests and twelve keyboard-layout child scenarios; ten opt-in checks
were skipped. Strict release all-target/all-feature Clippy and app-identity
fixtures passed. The signed, notarized bundle's isolated HUD preview was visibly
present over native fullscreen TextEdit. This exercises the packaged renderer,
not microphone capture or an installed-app update. The preview and disposable
TextEdit process were stopped afterward.

## Gaps And Constraints

- **Physical input:** Fn/Globe, assistive input, remapped modifier sides, and
  cross-tap delivery ordering need their own native evidence. A passing Option
  fixture does not cover every binding. See [shortcut recovery](recovery.md).
- **Full user path:** source connections and individual checks do not prove the
  complete path on a user's Mac. Retain action-to-result evidence for hold,
  lock, Escape, and a second capture during older processing in a controlled
  target app. Observe insertion and input responsiveness, not just the HUD.
- **Microphone modes:** verify onset and idle closure separately for Keep ready
  and Release when idle on an identified device. The macOS microphone indicator
  while Keep ready is idle is expected, not proof of recording.
- **Voice Action is distinct:** its
  [cold modifier-order gap](voice-action.md#cold-modifier-order) is not covered by
  ordinary cold-dictation checks.
- **Incremental transcription:** `dictate.incremental` has benchmark and
  synthetic evidence only. Real microphones, room noise, Bluetooth inputs, and
  inference under load have not been observed with piece handoff during capture.
- **Paste consumption:** current paste policy waits a fixed interval rather than
  acknowledging target consumption. Internal ordering does not establish
  correctness for an arbitrarily slow target application.
- **Cloud transcription:** requests are not cancellable mid-upload, so Escape
  after commit to the worker still waits for the HTTP response (bounded by
  `timeout_seconds`) before discarding it. Provider calls are unverified here.
- **Native checks require deliberate setup:** permissions, model, keyboard,
  microphone, output device, and a disposable text target must be recorded. This
  map does not authorize operating the installed app or injecting global input.
