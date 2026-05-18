#!/usr/bin/env python3
"""A small but real scikit-learn training run, used as the workload that
`nami run` wraps and schedules in the demo (see `examples/demo.sh`).

Deliberately self-contained: trains a RandomForest on scikit-learn's
*bundled* digits dataset (no network, no large download), prints
progress to stdout, and propagates a meaningful exit code — 0 on
success, 1 if accuracy falls below a sanity floor — so the demo can show
`nami` faithfully forwarding the child's exit status.
"""

import sys
import time

from sklearn.datasets import load_digits
from sklearn.ensemble import RandomForestClassifier
from sklearn.metrics import accuracy_score
from sklearn.model_selection import train_test_split


def main() -> int:
    print("[train] loading bundled digits dataset...", flush=True)
    X, y = load_digits(return_X_y=True)
    X_tr, X_te, y_tr, y_te = train_test_split(
        X, y, test_size=0.2, random_state=42, stratify=y
    )
    print(
        f"[train] {X_tr.shape[0]} train / {X_te.shape[0]} test samples, "
        f"{X.shape[1]} features",
        flush=True,
    )

    clf = RandomForestClassifier(n_estimators=300, random_state=42, n_jobs=-1)
    t0 = time.time()
    clf.fit(X_tr, y_tr)
    elapsed = time.time() - t0

    acc = accuracy_score(y_te, clf.predict(X_te))
    print(
        f"[train] fit complete in {elapsed:.1f}s; test accuracy = {acc:.4f}",
        flush=True,
    )

    if acc < 0.80:
        print("[train] accuracy below sanity floor (0.80) — failing", file=sys.stderr)
        return 1

    print("[train] OK", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
