"""
pytest-xdist assigns each worker a unique ID via the PYTEST_XDIST_WORKER env var: "gw0", "gw1", …  We map each worker to a non-overlapping 100-port band so parallel test processes never try to bind the same port.

Within a band, ports are handed out sequentially.  Before returning a port we do a quick socket.bind() probe to confirm the OS considers it free; if it is already occupied we skip it.

Thread-safety is provided by threading.Lock so multiple fixtures running concurrently inside one worker (pytest-asyncio, etc.) don't race.
"""

from __future__ import annotations

import os
import socket
import threading
from typing import List

_BAND_SIZE = 100
_BASE_PORT = 16000
_MAX_WORKERS = 50          # 50 workers × 100 ports = ports up to 20999

# Error to be raised when the ports are exhausted
class PortExhaustedError(RuntimeError):
    pass

# This class is used to assign free OS TCP ports.
class PortPool:

    def __init__(self) -> None:
        worker_id = os.environ.get("PYTEST_XDIST_WORKER", "gw0")
        try:
            worker_index = int(worker_id.lstrip("gw"))
        except ValueError:
            worker_index = 0

        if worker_index >= _MAX_WORKERS:
            raise PortExhaustedError(
                f"Worker index {worker_index} exceeds max supported workers "
                f"({_MAX_WORKERS}).  Increase _MAX_WORKERS in port_pool.py."
            )

        self._band_start: int = _BASE_PORT + worker_index * _BAND_SIZE
        self._band_end: int = self._band_start + _BAND_SIZE - 1
        self._next: int = self._band_start
        self._lock: threading.Lock = threading.Lock()

    # Allocates free required number of TCP ports from workers band
    def allocate(self, count: int = 1) -> List[int]:
        if count < 1:
            raise ValueError(f"count must be >= 1, got {count}")

        with self._lock:
            ports: List[int] = []
            candidate = self._next

            while len(ports) < count:
                if candidate > self._band_end:
                    raise PortExhaustedError(
                        f"Port band {self._band_start}–{self._band_end} exhausted "
                        f"(worker={os.environ.get('PYTEST_XDIST_WORKER', 'none')}).  "
                        f"Reduce num_braidpool_nodes or increase _BAND_SIZE."
                    )
                if self._is_free(candidate):
                    ports.append(candidate)
                candidate += 1

            self._next = candidate
            return ports

    # Helper function to check if the port is free
    @staticmethod
    def _is_free(port: int) -> bool:
        """Return True if the OS will let us bind TCP *port* on 127.0.0.1."""
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
            try:
                sock.bind(("127.0.0.1", port))
                return True
            except OSError:
                return False

    @property
    def band(self) -> tuple[int, int]:
        """Return (start, end) of this worker's port band (inclusive)."""
        return (self._band_start, self._band_end)
