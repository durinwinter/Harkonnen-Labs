import unittest

from src.readtime import estimate


class ReadtimeUnitTest(unittest.TestCase):
    def test_estimate_has_minimum_one_minute(self) -> None:
        self.assertEqual(estimate(""), 1)
