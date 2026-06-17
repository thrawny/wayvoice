# Prompt eval suite

The prompt eval suite lives in `tests/prompt_eval.rs` and is run with:

```bash
just prompt-eval
```

It requires `GROQ_API_KEY` and is ignored by default so `just check` stays fast.

Prompt eval fixtures are intentionally kept out of git for now. By default the harness reads:

```text
~/.cache/wayvoice/prompt-eval/current/manifest.json
~/.cache/wayvoice/prompt-eval/current/*.wav
```

Override with:

```bash
WAYVOICE_PROMPT_EVAL_DIR=/path/to/eval-dir just prompt-eval
```

Each manifest case maps a WAV file to expected text and key terms. The harness compares prompt variants (`none`, keywords, fragments, spoken examples, and noisy context negative control) and writes JSONL results to:

```text
target/prompt-eval/current.jsonl
```
