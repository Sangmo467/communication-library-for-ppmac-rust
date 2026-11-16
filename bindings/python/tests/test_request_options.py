"""验证默认透传、显式分类与只读池的本地选项映射，不连接控制器。"""
import unittest
from pmac import core


class RequestOptionsTests(unittest.TestCase):
    def test_default_and_explicit_modes(self):
        self.assertEqual(core.options().flags, 0)
        self.assertEqual(core.options({"passthrough": True}).flags, 0)
        self.assertEqual(core.options({"passthrough": False}).flags, 8)
        self.assertEqual(core.options({"infinite": True, "passthrough": False}).flags, 9)

    def test_read_pool_implies_classification(self):
        for passthrough in (True, False):
            self.assertEqual(core.options({"read_pool": True, "passthrough": passthrough}).flags, 2)
        self.assertEqual(core.options({"read_pool": True, "infinite": True}).flags, 3)
    def test_reset_commands_get_thirty_seconds_only_without_explicit_budget(self):
        for command in ("$$$", "$$$***", " \t$$$\r\n", b"$$$", bytearray(b"$$$***")):
            with self.subTest(command=command):
                self.assertEqual(core._command_options(command, {}).timeout_ms, 30000)
                self.assertEqual(core._command_options(command, {"passthrough": False}).timeout_ms, 30000)
                for timeout_ms in (0, 5000, 45000):
                    self.assertEqual(core._command_options(command, {"timeout_ms": timeout_ms}).timeout_ms, timeout_ms)
                self.assertEqual(core._command_options(command, {"infinite": True}).flags & 1, 1)
                self.assertEqual(core._command_options(command, {"infinite": True}).timeout_ms, 5000)
        for command in ("Sys.ServoCount", "$$$ Sys.ServoCount", "$$$\nSys.ServoCount", "$$$**"):
            self.assertEqual(core._command_options(command, {}).timeout_ms, 5000)
class ShutdownBudgetTests(unittest.TestCase):
    def test_invalid_close_budgets_are_rejected_before_native_calls(self):
        # Objects deliberately lack native handles: validation must happen first.
        runtime = core.Runtime.__new__(core.Runtime)
        client = core.Client.__new__(core.Client)
        for value in (-1, 2**32, 1.5, "1000", None):
            for method in (runtime.shutdown, client.close):
                with self.subTest(value=value, method=method.__qualname__):
                    with self.assertRaises((TypeError, ValueError)):
                        method(value)