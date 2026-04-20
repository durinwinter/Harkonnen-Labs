from src.readtime import estimate


def test_estimate_has_minimum_one_minute():
    assert estimate("") == 1
