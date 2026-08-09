# Prompt eval suite

The prompt eval suite lives in `tests/prompt_eval.rs` and is run with:

```bash
just prompt-eval    # Groq prompt variants
just provider-eval  # Groq vs ElevenLabs provider comparison
```

It requires provider API keys and is ignored by default so `just check` stays fast.

- `prompt_variants_eval_current_transcript_fixtures` requires `GROQ_API_KEY`.
- `provider_eval_current_transcript_fixtures` requires `GROQ_API_KEY` and `ELEVENLABS_API_KEY` (or `ELEVEN_LABS_API_KEY`).

Prompt eval fixtures are intentionally kept out of git for now. By default the harness reads:

```text
~/.cache/wayvoice/prompt-eval/current/manifest.json
~/.cache/wayvoice/prompt-eval/current/*.wav
```

Override with:

```bash
WAYVOICE_PROMPT_EVAL_DIR=/path/to/eval-dir just prompt-eval
WAYVOICE_PROMPT_EVAL_DIR=/path/to/eval-dir just provider-eval
```

Each manifest case maps a WAV file to expected text and key terms. The harness compares prompt variants (`none`, keywords, fragments, spoken examples, and noisy context negative control) and writes JSONL results to:

```text
target/prompt-eval/current.jsonl
```

The provider eval reuses the same fixtures to compare:

- `groq_no_prompt`
- `groq_keywords`
- `elevenlabs_no_keyterms_no_verbatim`
- `elevenlabs_keyterms_no_verbatim`

and writes:

```text
target/prompt-eval/providers.jsonl
```

Provider eval reuses transcripts from the previous `providers.jsonl` run so repeated runs do not call Groq/ElevenLabs again. Force a fresh run with:

```bash
WAYVOICE_PROMPT_EVAL_REFRESH=1 just provider-eval
```
