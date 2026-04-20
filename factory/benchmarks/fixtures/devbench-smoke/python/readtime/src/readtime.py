def estimate(text: str) -> int:
    words = len(text.split())
    return max(1, words // 200)
