
# Jev Integration

Jev is the semantic decision layer.

## Input

Jev receives compact structured context rather than raw
shell command output.

Example:

```json
{
  "target": {
    "filename": "CBX_research.pdf",
    "extension": "pdf",
    "title": "Cannabioxepane Synthesis",
    "source_domain": "pubs.acs.org"
  },

  "candidates": [
    {
      "id": "D1",
      "path": "~/Documents/Research/Cannabinoids",
      "related_files": 8
    },

    {
      "id": "D2",
      "path": "~/Documents/Research",
      "related_files": 3
    }
  ]
}
```

## Output

Jev should return semantic decisions only.

```json
{
  "destination_id": "D1",
  "confidence": 0.92,
  "probabilities": {
    "D1": 0.92,
    "D2": 0.06,
    "NONE": 0.02
  }
}
```

The local policy engine decides whether the operation is
automatically executed.
