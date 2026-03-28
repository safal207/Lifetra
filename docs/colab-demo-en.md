# Lifetra: Colab Demo (EN)

> Goal: provide a reproducible, notebook-friendly walkthrough for running the Lifetra Python bindings in Google Colab.

Source demo notebook:  
`https://colab.research.google.com/drive/1p1WJeUtHt4_rnwkMNGbDTTz_mn0meuzi?hl=ru#scrollTo=pmJUCckOo7GS`

## What you will get

- a built and installed `lifetra_py` wheel in Colab;
- a working demo with `TrajectoryState`, `ResonanceState`, and `SynergyState`;
- quick correctness checks (`transition_count`, `is_aligned`, `is_productive`);
- a baseline for further experiments.

---

## Cell 1 — environment setup

```bash
cd /content
git clone https://github.com/safal207/Lifetra.git
cd Lifetra

curl https://sh.rustup.rs -sSf | sh -s -- -y
source "$HOME/.cargo/env"

python -m pip install --upgrade pip
python -m pip install maturin
```

## Cell 2 — build and install `lifetra_py`

```bash
cd /content/Lifetra/bindings/lifetra-py
source "$HOME/.cargo/env"
python -m maturin build --release --out ../../target/wheels
python -m pip install $(find ../../target/wheels -name "lifetra_py-*.whl" | head -n 1)
```

> If you execute commands in separate cells, run `source "$HOME/.cargo/env"` again before invoking `maturin`.

## Cell 3 — import check

```python
from lifetra_py import ResonanceState, SynergyState, Timestamp, StateTransition, TrajectoryState
print("lifetra_py import: OK")
```

## Cell 4 — demo scenario

```python
from lifetra_py import (
    ResonanceState,
    StateTransition,
    SynergyState,
    Timestamp,
    TrajectoryState,
    average_alignment,
    combined_score,
    is_aligned,
    is_productive,
    transition_count,
)

trajectory = TrajectoryState("emerging", 0.64, 0.51)
trajectory.add_transition(
    StateTransition("initialization", Timestamp(1_710_000_000), "concept takes coherent form")
)
trajectory.add_transition(
    StateTransition("prototype", Timestamp(1_710_086_400), "first external feedback arrives")
)

resonance = ResonanceState(0.80, 0.74, 0.69)
synergy = SynergyState(0.83, 0.71)

print(trajectory.summary())
print("transition_count:", transition_count(trajectory))
print("average_alignment:", average_alignment(resonance))
print("is_aligned(0.69):", is_aligned(resonance, 0.69))
print("combined_score:", combined_score(synergy))
print("is_productive(0.70):", is_productive(synergy, 0.70))
```

### Expected output behavior

- `transition_count` should be `2`;
- `is_aligned(0.69)` should return `True`;
- `is_productive(0.70)` should return `True`;
- trajectory summary should print normally.

## Cell 5 — optional smoke check

```bash
cd /content/Lifetra
python bindings/lifetra-py/examples/smoke_test.py
```

---

## Common issues

### `maturin: command not found`

```bash
python -m pip install maturin
```

### `cargo`/`rustc` unavailable in current cell

```bash
source "$HOME/.cargo/env"
```

### wheel not found

```bash
find /content/Lifetra/target/wheels -name "lifetra_py-*.whl"
```

### module import fails after install

Try `Runtime -> Restart runtime`, then rerun Cells 2–4.

---

## Related files in this repository

- `bindings/lifetra-py/examples/minimal_usage.py`
- `bindings/lifetra-py/examples/smoke_test.py`
- `examples/idea_evolution.rs`
