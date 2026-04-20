# UML Sequence

```mermaid
sequenceDiagram
    actor User
    User->>ReadtimeService: estimate(text)
    ReadtimeService-->>User: duration estimate
```
