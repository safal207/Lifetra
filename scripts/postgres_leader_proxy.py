#!/usr/bin/env python3
"""Tiny test-only TCP endpoint that routes each new connection to the current PostgreSQL leader.

The target file contains one `host:port` line. Existing connections are never migrated;
new Lifetra connections follow the target selected by the HA test harness.
"""

from __future__ import annotations

import argparse
import socket
import threading
from pathlib import Path

BUFFER_SIZE = 64 * 1024


def read_target(path: Path) -> tuple[str, int]:
    value = path.read_text(encoding="utf-8").strip()
    host, port = value.rsplit(":", 1)
    return host, int(port)


def pipe(source: socket.socket, destination: socket.socket) -> None:
    try:
        while True:
            data = source.recv(BUFFER_SIZE)
            if not data:
                break
            destination.sendall(data)
    except OSError:
        pass
    finally:
        try:
            destination.shutdown(socket.SHUT_WR)
        except OSError:
            pass


def handle(client: socket.socket, target_file: Path) -> None:
    backend: socket.socket | None = None
    try:
        host, port = read_target(target_file)
        backend = socket.create_connection((host, port), timeout=10)
        backend.settimeout(None)
        client.settimeout(None)
        upstream = threading.Thread(target=pipe, args=(client, backend), daemon=True)
        downstream = threading.Thread(target=pipe, args=(backend, client), daemon=True)
        upstream.start()
        downstream.start()
        upstream.join()
        downstream.join()
    except (OSError, ValueError):
        pass
    finally:
        try:
            client.close()
        except OSError:
            pass
        if backend is not None:
            try:
                backend.close()
            except OSError:
                pass


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--listen-host", default="127.0.0.1")
    parser.add_argument("--listen-port", type=int, required=True)
    parser.add_argument("--target-file", type=Path, required=True)
    args = parser.parse_args()

    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        listener.bind((args.listen_host, args.listen_port))
        listener.listen(128)
        while True:
            client, _ = listener.accept()
            threading.Thread(target=handle, args=(client, args.target_file), daemon=True).start()


if __name__ == "__main__":
    main()
