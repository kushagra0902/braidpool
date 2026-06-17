"""LIFO cleanup orchestration for Braidpool functional tests."""

from __future__ import annotations

import atexit
import logging
import os
import signal
import subprocess
import threading
from collections.abc import Callable
from types import FrameType

from test_framework.logging_utils import log_event, log_exception
from test_framework.process_utils import _terminate_process_group


_LOGGER = logging.getLogger(__name__)


class CleanupManager:
    """Register and run cleanup callbacks for a functional test process.

    Instances install ``atexit``, ``SIGTERM``, and ``SIGINT`` hooks during
    initialization. Construct this class from the main thread; CPython only
    allows signal handlers to be installed there.
    """

    def __init__(self, logger: logging.Logger | None = None) -> None:
        self._logger = logger or _LOGGER
        self._stack: list[tuple[str, Callable[[], None]]] = []
        self._lock = threading.Lock()
        self._ran = False
        self._previous_handlers: dict[int, signal.Handlers] = {}
        self._install_handlers()

    #The cleanup manager is a registrable that is attached to different modules later when running the test suite such as miner_manager. This allows us to register different callback fucntions in a LIFo stack.
    def register(self, fn: Callable[[], None], *, name: str | None = None) -> None:
        """Push a zero-argument cleanup callback onto the LIFO stack."""
        callback_name = name or _callable_name(fn)
        with self._lock:
            if self._ran:
                log_event(
                    self._logger,
                    "cleanup_register_after_run",
                    level=logging.WARNING,
                    name=callback_name,
                )
                return
            self._stack.append((callback_name, fn))
        log_event(
            self._logger,
            "cleanup_callback_registered",
            level=logging.DEBUG,
            name=callback_name,
        )

    def run_all(self) -> None:
        """Execute all registered callbacks in LIFO order exactly once."""
        with self._lock:
            if self._ran:
                return
            self._ran = True
            stack = list(self._stack)

        for name, fn in reversed(stack):
            try:
                log_event(self._logger, "cleanup_callback_running", name=name)
                fn()
                log_event(self._logger, "cleanup_callback_done", name=name)
            except Exception as exc:
                log_exception(self._logger, "cleanup_callback_failed", exc, name=name)

    def managed_process(
        self,
        process: subprocess.Popen,
        name: str,
        *,
        term_timeout: float = 3.0,
        kill_timeout: float = 1.0,
    ) -> None:
        """Register a callback that terminates a subprocess group on cleanup."""

        def _stop() -> None:
            _terminate_process_group(
                process,
                name=name,
                term_timeout=term_timeout,
                kill_timeout=kill_timeout,
                logger=self._logger,
            )

        self.register(_stop, name=f"process:{name}")

    #This installs all the handlers when the CleanupManger is initialized. It sets up a signal chaining mechaninsm so that when the test process is terminated due to signals it can gracefully exit after running all the cleanup callbacks.
    def _install_handlers(self) -> None:
        atexit.register(self.run_all)
        for signum in (signal.SIGTERM, signal.SIGINT):
            self._previous_handlers[signum] = signal.getsignal(signum)
            signal.signal(signum, self._signal_handler)

    def _signal_handler(self, signum: int, frame: FrameType | None) -> None:
        log_event(self._logger, "cleanup_signal_received", signal=signum)
        self.run_all()
        self._delegate_signal(signum, frame)

    def _delegate_signal(self, signum: int, frame: FrameType | None) -> None:
        previous = self._previous_handlers.get(signum, signal.SIG_DFL)
        if previous == signal.SIG_IGN:
            return
        if callable(previous):
            previous(signum, frame)
            return
        signal.signal(signum, signal.SIG_DFL)
        os.kill(os.getpid(), signum)


def _callable_name(fn: Callable[[], None]) -> str:
    return getattr(fn, "__qualname__", getattr(fn, "__name__", repr(fn)))
