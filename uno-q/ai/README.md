# Bob's AI development kit

## Start with command planning

Run **[A30_tool_planner.ipynb](A30_tool_planner.ipynb)** in your A30 Jupyter environment. This is the primary notebook following the decision to use a small language model to select existing desktop tools. The default training candidate is Qwen3-0.6B with thinking traces disabled. It has not been trained on the user's A30 or benchmarked on UNO Q yet.

The notebook runs a pretrained baseline, generates a synthetic starter dataset if you do not provide real traces, fine-tunes LoRA, evaluates generated next-tool answers on held-out workflow groups, and exports merged weights. No desktop actions execute from the notebook. The baseline and evaluation prompts do not include the target answer.

Send back:

- `bob-tool-planner-lora/run_summary.json`
- `bob-tool-planner-lora/heldout-results.json`
- Any error traceback if a cell fails.

Keep `bob-tool-planner-merged/` on the A30 machine for now. After reviewing results, we will convert/quantize and measure the trained model on UNO Q. Synthetic workflow scores are not live Chrome task-success rates.

## Architecture and the original GitHub references

```text
User request + current observation + recent tool results
                         ↓
             Language/tool planner on UNO Q
                         ↓ one structured tool call
             Windows session / mode controller
                         ↓
          Desktop executor → result / observation
                         └────────── back to planner
```

The [ai-desktop repository](https://github.com/MichaelDKulka/ai-desktop/tree/28dd1ba7d3f66fc9212f27a8f6c82d444af346e4) provides executable primitives, including `launch-chrome`, `hotkey`, `type`, `key` and screenshots. It does not supply the reasoning model or the complete Bob agent loop. Its MIT license permits reuse with its notice preserved when code is distributed. No upstream implementation is vendored here yet.

`tool_plan.py` validates a bounded tool vocabulary and maps calls to the existing upstream CLI argument arrays. It never invokes a shell or runs those arrays. `observe` uses Bob's existing in-memory screenshot path rather than upstream disk-backed screenshot storage.

Example next tool:

```json
{"tool":"hotkey","args":{"keys":"ctrl+l"}}
```

For “hey can you open stocks for nvidai”, the initial workflow is Chrome → focus address bar → type `NVIDIA stock` → Enter → observe → finish. In a production loop each next command must depend on the actual executor result and observation, not an assumed fixed delay or success.

**Integration status:** the current Windows executable still accepts normalized move/click actions only. Browser launching, text entry and keyboard tools must be wired into its native USB permit, foreground-window, Guide/Auto and Stop controls before this planner can execute them. Guide will show instructions; Auto will execute allowed tools. The new model is not connected to Auto mode.

OmniParser/vision can be added when identifying a visual target is necessary. It need not process every keyboard-driven browser step.

## Measured on the physical UNO Q

Pinned llama.cpp `b11869` (commit `d327bf1f0`), four CPU threads, 2048-token context, Q8 model/projector weights. Both pretrained SmolVLM candidates were downloaded with SHA-256 checks and tested on the actual board. Results are in `results/`.

| Model | Initial 1280×720 synthetic button tests | Text-only NVIDIA query with a constrained schema |
|---|---|---|
| SmolVLM 500M | 0/2 targets; 84.5–89.7 seconds per response | `nvidia stocks`, valid; 3.66 seconds |
| SmolVLM 256M | 0/2 targets; 75.5–76.6 seconds per response | Repetitive, incomplete output; 2.30 seconds; rejected |

These are tiny smoke tests, not general accuracy estimates. The 500M result demonstrates query extraction for one request; Chrome was not opened, no live stock data was retrieved, and no desktop input was injected. The smaller model was faster but failed the output check. Initial unconstrained JSON shape also failed; constraining properties fixed the 500M query shape without training.

The runtime and weights are staged under `/home/arduino/bob-ai/`. The ordinary `bob-butler` boot service is unchanged and still reports `ai_ready: false`. Benchmark servers are temporary loopback-only processes and are terminated when the runner finishes. The board's clock was corrected and NTP enabled so HTTPS certificate verification could remain enabled.

## Reproduce

`provision_benchmark.py` pins runtime/model revisions and checks every download. It needs Python 3.12+ for safe tar extraction and a correct clock for HTTPS. The Q has Python 3.13. When downloading on the laptop, use its `download()` helper then copy verified files over USB; large USB transfers on this board are slow.

```sh
python3 provision_benchmark.py
python3 benchmark.py --only chat-intent --output benchmark-intent-schema.json
```

For the original unstructured/high-resolution image test:

```sh
python3 benchmark.py --plain-json --fixtures fixtures/high-resolution --output initial-q8-1280.json
```

The default fixtures are now 640×360 to match Bob's current observation width. No new 640-pixel visual scores are claimed. `model_adapter.py` is a strict local-inference adapter for query/point experiments; it never executes an action. `A30_finetune.ipynb` is the optional visual-grounding experiment, not the primary command-planner notebook.

Local tests:

```powershell
python -m unittest discover -s uno-q/ai -p "test_*.py" -v
```

Training uses pretrained weights; it is not training an LLM from scratch. Real demonstration data, full task evaluation and on-board latency checks remain necessary before autonomous use.
