import json
from pathlib import Path
import unittest

from pmac import CoordStatus, MotorStatus


class LegacyStatusMigrationTests(unittest.TestCase):
    def test_every_legacy_status_accessor_matches_the_frozen_bit_definition(self):
        fixture = json.loads(Path(__file__).with_name("legacy-status-fields.json").read_text(encoding="utf-8"))
        maximum = (1 << 64) - 1
        classes = {"MotorStatus": MotorStatus, "CoordStatus": CoordStatus}
        for field in fixture["fields"]:
            width, shift = field["width"], field["shift"]
            mask = (1 << width) - 1
            raw_values = [0, maximum, mask << shift, maximum ^ (mask << shift)]
            raw_values += [value << shift for value in range(mask + 1)]
            for raw in raw_values:
                with self.subTest(status=field["status"], field=field["python"], raw=raw):
                    status = classes[field["status"]](raw)
                    actual = getattr(status, field["python"])
                    self.assertEqual(actual, (raw >> shift) & mask)
                    self.assertIsInstance(actual, bool if width == 1 else int)
                    self.assertEqual(status.raw, raw)


if __name__ == "__main__":
    unittest.main()
