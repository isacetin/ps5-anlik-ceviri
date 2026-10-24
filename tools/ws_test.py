#!/usr/bin/env python3
"""PS5 Çeviri overlay'ini Mac tarafı olmadan WebSocket üzerinden test eder.

Kullanım:
    pip3 install websockets
    python3 tools/ws_test.py TV_IP [--port 8765]

Akış:
    1. Aynı listeyi saniyede bir gönderir (heartbeat): yazı ekranda kalır.
    2. Listeyi değiştirir (iki blok, biri çok satırlı): ekran yeni listeyle değişir.
    3. Boş liste gönderir: overlay hemen temizlenir.
    4. Tek mesaj gönderip susar: 3 sn sonra yazı kendiliğinden kaybolur (timeout).
    5. Eski seq'li mesaj gönderir: yok sayılmalı, ekranda hiçbir şey çıkmamalı.
    6. Bozuk JSON gönderir: yok sayılmalı, servis çökmemeli.
"""

import argparse
import asyncio
import json

import websockets

DOOR = {"id": "door", "x": 0.20, "y": 0.80, "w": 0.60, "h": 0.08, "lineH": 0.04, "text": "KAPI KİLİTLİ"}
HINT = {
    "id": "hint",
    "x": 0.05,
    "y": 0.10,
    "w": 0.30,
    "h": 0.16,
    "lineH": 0.035,
    "text": "Anahtarı bulmak için kuzeydeki bekçiyle konuş. Çğıİöşü ÇĞÖŞÜ",
}


class Sender:
    def __init__(self, ws):
        self.ws = ws
        self.seq = 0

    async def send(self, blocks, seq=None):
        if seq is None:
            self.seq += 1
            seq = self.seq
        await self.ws.send(json.dumps({"v": 1, "seq": seq, "blocks": blocks}, ensure_ascii=False))

    async def heartbeat(self, blocks, seconds):
        for _ in range(seconds):
            await self.send(blocks)
            await asyncio.sleep(1)


async def main(host, port):
    url = f"ws://{host}:{port}"
    print(f"Bağlanılıyor: {url}")
    async with websockets.connect(url) as ws:
        sender = Sender(ws)

        print("1) Heartbeat: aynı liste 5 sn boyunca saniyede bir -> 'KAPI KİLİTLİ' ekranda kalmalı")
        await sender.heartbeat([DOOR], 5)

        print("2) Liste değişti: iki blok 5 sn -> sol üstte çok satırlı ipucu da görünmeli")
        await sender.heartbeat([DOOR, HINT], 5)

        print("3) Boş liste -> overlay hemen temizlenmeli")
        await sender.send([])
        await asyncio.sleep(2)

        print("4) Tek mesaj, heartbeat yok -> yazı 3 sn sonra kendiliğinden kaybolmalı")
        await sender.send([DOOR])
        await asyncio.sleep(4)

        print("5) Eski seq (1) -> yok sayılmalı, ekranda bir şey çıkmamalı")
        await sender.send([HINT], seq=1)
        await asyncio.sleep(2)

        print("6) Bozuk JSON -> yok sayılmalı, servis çalışmaya devam etmeli")
        await ws.send("{bozuk json")
        await asyncio.sleep(1)

    print("Bitti.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="PS5 Çeviri WebSocket testi")
    parser.add_argument("host", help="TV'nin IP adresi")
    parser.add_argument("--port", type=int, default=8765)
    args = parser.parse_args()
    asyncio.run(main(args.host, args.port))
