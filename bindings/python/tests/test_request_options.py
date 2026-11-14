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