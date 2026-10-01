# Sends NetFlow v5 packets, as a router's exporter would, to the agent's listener.
import socket, struct, time
s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
ip = lambda a: socket.inet_aton(a)
flows = [
    ("10.1.0.5", "10.2.0.9", 51000, 443, 6, 0x1b, 120, 96000),
    ("10.1.0.5", "10.2.0.9", 51001, 443, 6, 0x1b, 30, 2400),
    ("10.1.0.7", "8.8.8.8", 53124, 53, 17, 0, 2, 150),
    ("10.2.0.9", "10.1.0.5", 443, 51000, 6, 0x1a, 200, 250000),
]
seq = 0
for burst in range(6):
    now = time.time()
    uptime = 3_600_000 + burst * 5000
    header = struct.pack("!HHIIIIBBH", 5, len(flows), uptime, int(now), int((now % 1) * 1e9), seq, 0, 1, 0)
    body = b""
    for src, dst, sport, dport, proto, flags, pkts, octets in flows:
        body += ip(src) + ip(dst) + ip("10.0.0.1") + struct.pack("!HHIIIIHHBBBBHHBBH", 1, 2, pkts, octets, uptime - 4000, uptime - 100, sport, dport, 0, flags, proto, 0, 64512, 64513, 24, 24, 0)
    s.sendto(header + body, ("127.0.0.1", 2055))
    seq += len(flows)
    time.sleep(3)
print("netflow sent")
