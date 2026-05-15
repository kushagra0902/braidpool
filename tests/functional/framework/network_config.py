"""
network_config.py — Tunable parameters for a single test run.

Every test file declares a subclass of BraidpoolTestCase and can override `config` with a NetworkConfig instance.

Port fields are intentionally left empty (default_factory=list) as they are filled in by PortPool during NodeManager.setup()
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import List, Optional


@dataclass
class NetworkConfig:
    # Number of braidpool nodes in the network
    num_braidpool_nodes: int = 1
    # Number of cpu miners in the network
    num_cpu_miners: int = 0

    # Path to the IPC unix socket.  If None, a temp path is generated.
    bitcoin_ipc_socket: Optional[str] = None

    # Blocks mined after Bitcoin Core starts for maturing the coinbase and making it usable.
    initial_blocks: int = 101

    # Braidpool port lists (populated by PortPool, not by the test)
    braidpool_bind_ports: List[int] = field(default_factory=list)

    braidpool_rpc_ports: List[int] = field(default_factory=list)

    braidpool_stratum_ports: List[int] = field(default_factory=list)

    # Bitcoin/Braidpool network.  regtest | cpunet | signet.
    network: str = "cpunet"

    # Binary locations (overridden by BITCOIN_NODE_PATH / BRAIDPOOL_BIN_PATH
    # env vars or auto-discovered — see framework/__init__.py)
    bitcoin_bin_path: str = "bitcoin-node"
    braidpool_bin_path: str = "target/debug/node"
    minerd_bin_path: str = "minerd"

    # Timeouts (seconds)
    startup_timeout_braidpool: float = 15.0
    startup_timeout_btc: float = 75.0
    startup_timeout_minerd: float = 15.0

    peer_connection_timeout: float = 15.0

    bead_propagation_timeout: float = 60.0

    # Logging & test isolation
    log_level: str = "INFO"
    keep_logs_on_success: bool = False
    datadir_prefix: str = "/tmp/bp-test"

    # Reproducibility
    random_seed: int = 42
