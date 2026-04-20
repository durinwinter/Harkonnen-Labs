from src.readtime import estimate


def test_example_usage():
    assert estimate("hello world") == 1
