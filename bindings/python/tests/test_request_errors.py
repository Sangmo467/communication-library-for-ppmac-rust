import ctypes as C
from types import SimpleNamespace
import unittest
from pmac import core


class ErrorNative:
    def __init__(self, mode):
        self.mode = mode

    def copy(self, local, info, output, capacity, required):
        if self.mode == 2:
            return -2
        status, message = ((-22, b'regex parse error: unclosed character class') if self.mode == 0
                           else (-1, b'unrelated last error')) if local else (
                               (0, b'') if self.mode == 0 else (-10, b'stdin:1:1: error #31: Buffer closed due to another thread:'))
        info._obj.status, info._obj.request_id, info._obj.certainty = status, 0 if local else 7, 0 if local else 2
        required._obj.value = len(message)
        if capacity < len(message):
            return -11
        if message:
            C.memmove(output, message, len(message))
        return 0

    def pmac_copy_last_error(self, *args):
        return self.copy(True, *args)

    def pmac_request_copy_error(self, request, *args):
        return self.copy(False, *args)

    def pmac_request_copy_match(self, *args):
        return -22 if self.mode == 0 else -10

    def pmac_request_cancel(self, request):
        return 0

    pmac_request_release = pmac_request_cancel


class RequestErrorTests(unittest.TestCase):
    def test_request_integer_arguments_are_checked_before_native_calls(self):
        runtime = SimpleNamespace(lib=ErrorNative(0), _check_pid=lambda: None)
        with core.Request(runtime, 7) as request:
            for value in (-1, 2**32):
                with self.assertRaises(ValueError):
                    request.wait(value)
                with self.assertRaises(ValueError):
                    request.match("x", value)
            for value in (1.5, "1"):
                with self.assertRaises(TypeError):
                    request.wait(value)
                with self.assertRaises(TypeError):
                    request.match("x", value)
    def test_local_diagnostic_request_fallback_and_nonempty_fallback(self):
        for mode, expected in enumerate(('unclosed character class', 'error #31', 'failed')):
            with self.subTest(mode=mode):
                native = ErrorNative(mode)
                runtime = SimpleNamespace(lib=native, _check_pid=lambda: None)
                with core.Request(runtime, 7) as request:
                    with self.assertRaises(core.PmacError) as raised:
                        request.match('[')
                    self.assertEqual(raised.exception.status, -22 if mode == 0 else -10)
                    self.assertIn(expected, str(raised.exception))
                    self.assertEqual(raised.exception.request_id, 7)
                    if mode != 2:
                        self.assertEqual(raised.exception.certainty, 2)


if __name__ == '__main__':
    unittest.main()