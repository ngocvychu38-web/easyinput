#!/usr/bin/env python3
"""Short real-board transport check. Discards PCM; never records speech or reads keys."""
import collections
import json
import secrets
import socket
import struct
import time


def main():
    # Match the application port without printing the rest of its configuration.
    from pathlib import Path
    config = json.loads((Path.home() / 'Library/Application Support/pro.easyinput.desktop.intel/config.json').read_text())
    port = config['keyboard']['wifi']['audioPort']
    wire = secrets.randbits(64)
    sequence = 0
    peer = None
    stats = dict(startAck=False, stopAck=False, validFrames=0, separateAudioPort=False, sequenceGaps=0)
    previous = None
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
        sock.bind(('0.0.0.0', port))
        sock.settimeout(8)
        deadline = time.monotonic() + 8
        while time.monotonic() < deadline:
            packet, address = sock.recvfrom(4096)
            if packet[:4] == b'EIHB':
                peer = address
                break
        if peer is None:
            raise RuntimeError('No board heartbeat')

        def control(action):
            nonlocal sequence
            sequence += 1
            packet = bytearray(36)
            packet[:4] = b'EICC'
            packet[4:6] = bytes([1, action])
            struct.pack_into('<QI', packet, 8, wire, sequence)
            sock.sendto(packet, peer)
            return sequence

        try:
            start_sequence = control(1)
            deadline = time.monotonic() + 6
            keepalive = time.monotonic() + 1
            sock.settimeout(.2)
            while time.monotonic() < deadline:
                if time.monotonic() >= keepalive:
                    control(3)
                    keepalive = time.monotonic() + 1
                try:
                    packet, address = sock.recvfrom(4096)
                except TimeoutError:
                    continue
                if address[0] != peer[0]:
                    continue
                if packet[:4] == b'EICA' and len(packet) == 20 and address == peer:
                    if packet[5:7] == bytes([1, 0]) and struct.unpack_from('<QI', packet, 8) == (wire, start_sequence):
                        stats['startAck'] = True
                if (len(packet) == 672 and packet[:8] == b'EIAU\x02\x20\x01\x01'
                    and struct.unpack_from('<Q', packet, 8)[0] == wire
                    and struct.unpack_from('<I', packet, 20)[0] == 16000
                    and struct.unpack_from('<HH', packet, 28) == (320, 640)):
                    current = struct.unpack_from('<I', packet, 16)[0]
                    if previous is not None and ((current - previous) & 0xffffffff) != 1:
                        stats['sequenceGaps'] += 1
                    previous = current
                    stats['validFrames'] += 1
                    stats['separateAudioPort'] |= address[1] != peer[1]
        finally:
            stop_sequence = control(2)
            deadline = time.monotonic() + 2
            while time.monotonic() < deadline:
                try:
                    packet, address = sock.recvfrom(4096)
                except TimeoutError:
                    continue
                if (address == peer and len(packet) == 20 and packet[:4] == b'EICA'
                    and packet[5:7] == bytes([2, 0])
                    and struct.unpack_from('<QI', packet, 8) == (wire, stop_sequence)):
                    stats['stopAck'] = True
                    break
    print(json.dumps(stats))
    if not (stats['startAck'] and stats['stopAck'] and stats['validFrames'] >= 100):
        raise SystemExit('FAIL: board transport incomplete')


if __name__ == '__main__':
    main()
