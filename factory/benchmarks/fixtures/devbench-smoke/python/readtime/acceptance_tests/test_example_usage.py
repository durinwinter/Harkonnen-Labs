import unittest

from src.readtime import estimate


class ExampleUsageTest(unittest.TestCase):
    def test_example_usage(self) -> None:
        self.assertEqual(estimate("hello world"), 1)
