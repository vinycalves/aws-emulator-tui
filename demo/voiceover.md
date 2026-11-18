# Voiceover script (English)

Two versions: a 20-second cut that matches the rendered `docs/demo.mp4`, and a
60-second extended cut for YouTube/LinkedIn. Record close to the microphone,
normalize to about −16 LUFS, and leave 200–300 ms of room before the first word.

## 20-second cut

| Time | Voiceover |
| --- | --- |
| 0:00–0:03 | This is **emutui** — a terminal console for AWS Emulator. |
| 0:03–0:06 | AWS Emulator gives you the AWS APIs locally, but no interface. |
| 0:06–0:10 | Here is S3: buckets, objects, and a text preview, without leaving the terminal. |
| 0:10–0:13 | Press **o** on a DynamoDB table to see its items; **enter** opens the JSON. |
| 0:13–0:17 | Browse all 436 services, run `ListQueues`, and the table appears with pagination. |
| 0:17–0:20 | Diagnostics, read-only mode, and the emulator lifecycle — one Rust binary, MIT. |

## 60-second extended cut

| Time | Voiceover |
| --- | --- |
| 0:00–0:04 | AWS Emulator gives you the AWS APIs locally — fast, offline, no account. |
| 0:04–0:08 | What it does not give you is a console. That is the gap **emutui** fills. |
| 0:08–0:14 | It attaches to the emulator as a normal AWS client: endpoint, keys, and certificate discovered automatically. |
| 0:14–0:21 | The Explorer gives you live tables for sixteen services. Here is S3: buckets, objects, and a text preview. |
| 0:21–0:28 | Drill into a DynamoDB table with **o**; the items show up with their typed attributes unwrapped. |
| 0:28–0:36 | **Browse** is the long tail: all 436 services and 19,607 operations, with automatic table extraction and pagination. |
| 0:36–0:43 | The API console is protocol-aware and SigV4-signed: JSON, query, EC2 query, and REST-JSON. |
| 0:43–0:50 | Actions are one keystroke: send an SQS message, publish to SNS, invoke a Lambda, start a state machine. |
| 0:50–0:56 | Diagnostics, read-only mode, JSON export, and a command palette keep it practical for daily use. |
| 0:56–1:00 | One Rust binary, MIT licensed. Link below. |

## Delivery notes

- Emphasize the concrete nouns: S3, DynamoDB, SQS, 436 services.
- Pause after “no interface” and after “the gap emutui fills.”
- Do not call it an official AWS tool. It is an independent project.
- If you use an AI voice, disclose it in the video description.
- For LinkedIn, add burned-in captions; most viewers start muted.

## Music

Use a quiet, minimal electronic bed at −24 to −30 LUFS under the voice, or no
music. Do not use copyrighted tracks.