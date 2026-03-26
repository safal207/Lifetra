# Lifetra: оформление Colab-демо в документацию

> Этот документ переводит demo-сценарий в пошаговый, воспроизводимый формат для Google Colab.

## Что показывает демо

Демо проверяет, что Python может использовать Rust-модель Lifetra через модуль `lifetra_py`:

- сборка Python wheel через `maturin`;
- импорт типов (`Timestamp`, `StateTransition`, `TrajectoryState`, `ResonanceState`, `SynergyState`);
- базовые операции над траекторией, резонансом и синергией;
- проверка вспомогательных функций (`average_alignment`, `combined_score`, `is_aligned`, `is_productive`, `transition_count`).

## 1) Подготовка Colab-окружения

Выполните в первой ячейке:

```bash
git clone https://github.com/safal207/Lifetra.git
cd Lifetra
curl https://sh.rustup.rs -sSf | sh -s -- -y
source "$HOME/.cargo/env"
python -m pip install --upgrade pip
python -m pip install maturin
```

## 2) Сборка и установка `lifetra_py`

```bash
cd /content/Lifetra/bindings/lifetra-py
source "$HOME/.cargo/env"
python -m maturin build --release --out ../../target/wheels
python -m pip install $(find ../../target/wheels -name "lifetra_py-*.whl" | head -n 1)
```

> Если выполняете команды в разных ячейках, повторяйте `source "$HOME/.cargo/env"` перед `maturin`.

## 3) Проверка демо в Python

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

Ожидаемое поведение:

- отображается текстовое summary траектории;
- `transition_count` возвращает `2`;
- `is_aligned(0.69)` возвращает `True`;
- `is_productive(0.70)` возвращает `True`.

## 4) Частые проблемы

### `maturin: command not found`
Убедитесь, что команда установки выполнилась без ошибки:

```bash
python -m pip install maturin
```

### Rust не подхватился в новой ячейке
Повторите:

```bash
source "$HOME/.cargo/env"
```

### wheel не найден
Проверьте, что сборка прошла успешно:

```bash
find /content/Lifetra/target/wheels -name "lifetra_py-*.whl"
```

## 5) Где смотреть расширенные примеры

- `bindings/lifetra-py/examples/minimal_usage.py`
- `bindings/lifetra-py/examples/smoke_test.py`
- `examples/idea_evolution.rs`

