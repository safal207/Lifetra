# Lifetra: Colab-демо (RU)

[![Open In Colab](https://colab.research.google.com/assets/colab-badge.svg)](https://colab.research.google.com/github/safal207/Lifetra/blob/main/notebooks/colab_demo.ipynb)

> Цель: превратить демо в воспроизводимую инструкцию для Google Colab, которую можно пройти без локальной настройки окружения.

Исходный demo-ноутбук:  
`https://colab.research.google.com/drive/1p1WJeUtHt4_rnwkMNGbDTTz_mn0meuzi?hl=ru#scrollTo=pmJUCckOo7GS`

## Что вы получите после прохождения

- собранный и установленный Python-модуль `lifetra_py` (через Rust + maturin);
- рабочий пример с `TrajectoryState`, `ResonanceState` и `SynergyState`;
- быстрые проверки корректности (`transition_count`, `is_aligned`, `is_productive`);
- готовую основу для экспериментов в вашем Colab.

---

## Ячейка 1 — подготовка окружения

```bash
# clone
cd /content
git clone https://github.com/safal207/Lifetra.git
cd Lifetra

# rust toolchain
curl https://sh.rustup.rs -sSf | sh -s -- -y
source "$HOME/.cargo/env"

# python tooling
python -m pip install --upgrade pip
python -m pip install maturin
```

## Ячейка 2 — сборка и установка `lifetra_py`

```bash
cd /content/Lifetra/bindings/lifetra-py
source "$HOME/.cargo/env"
python -m maturin build --release --out ../../target/wheels
python -m pip install $(find ../../target/wheels -name "lifetra_py-*.whl" | head -n 1)
```

> Если вы запускаете команды в разных ячейках, повторяйте `source "$HOME/.cargo/env"` перед `maturin`.

## Ячейка 3 — проверка импорта

```python
from lifetra_py import ResonanceState, SynergyState, Timestamp, StateTransition, TrajectoryState
print("lifetra_py import: OK")
```

## Ячейка 4 — демо-сценарий

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

### Ожидаемый результат

- `transition_count` равен `2`;
- `is_aligned(0.69)` возвращает `True`;
- `is_productive(0.70)` возвращает `True`;
- выводится summary траектории.

## Ячейка 5 — быстрый smoke check

```bash
cd /content/Lifetra
python bindings/lifetra-py/examples/smoke_test.py
```

---

## Частые проблемы

### `maturin: command not found`

```bash
python -m pip install maturin
```

### `cargo`/`rustc` не видны в текущей ячейке

```bash
source "$HOME/.cargo/env"
```

### wheel не найден

```bash
find /content/Lifetra/target/wheels -name "lifetra_py-*.whl"
```

### После установки модуль не импортируется

Иногда помогает перезапустить runtime (`Runtime -> Restart runtime`) и повторить ячейки 2–4.

---

## Полезные файлы в репозитории

- `bindings/lifetra-py/examples/minimal_usage.py`
- `bindings/lifetra-py/examples/smoke_test.py`
- `examples/idea_evolution.rs`


## Переключение языка

- English version: `docs/colab-demo-en.md`
- Notebook в репозитории: `notebooks/colab_demo.ipynb`
