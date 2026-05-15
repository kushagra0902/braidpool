from __future__ import annotations

import sys
from pathlib import Path

import pytest

# Ensure `framework` package is importable from any working directory by making the path global.
_HERE = Path(__file__).parent.resolve()
if str(_HERE) not in sys.path:
    sys.path.insert(0, str(_HERE))

#Defining the markers so that they can be used in the test code
def pytest_configure(config: pytest.Config) -> None:
    config.addinivalue_line(
        "markers",
        "requires_bitcoin: test requires a Bitcoin Core binary "
        "(deselect with -m 'not requires_bitcoin')",
    )
    config.addinivalue_line(
        "markers",
        "requires_braidpool: test requires a Braidpool node binary "
        "(deselect with -m 'not requires_braidpool')",
    )
    config.addinivalue_line(
        "markers",
        "requires_miner: test requires a cpuminer (minerd) binary "
        "(deselect with -m 'not requires_miner')",
    )

# This hook is used to attach the result of each phase of a pytest to the test item itself, so that is can be accessed later
@pytest.hookimpl(tryfirst=True, hookwrapper=True)
def pytest_runtest_makereport(
    item: pytest.Item, call: pytest.CallInfo
) -> None:
    outcome = yield
    rep = outcome.get_result()
    setattr(item, f"rep_{rep.when}", rep)
