# receiver: odbiornik Miracast na małej płytce (eksperymentalny)

`wfd_sink.py` to minimalny odbiornik Wi-Fi Display R1 (sink) dla płytki z Linuksem, HDMI
i sprzętowym dekoderem H.264. Sprawdzony na **Raspberry Pi Zero W** (ARMv6, 1 rdzeń):
ze źródłem z tego repo (`wfd-source`) pokazuje 1080p30 z dźwiękiem LPCM. Służy jako
odbiornik testowy z pełnym wglądem w RTSP i dekoder, nie jako zamiennik telewizora.

## Jak działa

- **P2P przez NetworkManagera**, obok zwykłego Wi-Fi: SSH po tym samym radiu działa dalej.
  NM usuwa (D-Bus `RemoveInterface`) każdą grupę P2P, której sam nie założył, więc grupę
  zakłada NM. Na żądanie negocjacji od źródła odbiornik aktywuje połączenie `wifi-p2p`
  do tego źródła. NM nie respektuje `p2p_go_intent`, rola wychodzi różnie i obie działają:
  - GO: NM sam serwuje DHCP (`shared`), adres źródła jest w pliku dzierżaw NM;
  - klient: źródło jest serwerem DHCP (`dhcp_server_identifier`).
- **RTSP**: odbiornik łączy się z `<źródło>:7236`, odpowiada na M1–M4 i wysyła M2, M6 i M7.
  Oferuje H.264 CBP do 1080p30 (CEA 640x480p60, 720p30/60, 1080p30) i LPCM 48 kHz stereo.
- **Obraz**: `udpsrc` (z obciętym nagłówkiem RTP) `→ tsdemux → v4l2h264dec → kmssink`,
  a dźwięk przez `dvdlpcmdec → alsasink` (HDMI). Potok startuje razem z grupą P2P, bo na
  Zero W `gst-launch` potrzebuje około 7 s.
- **Log**: jeden obiekt JSON w linii na stderr (`{"event": ...}`).

Tryb `--p2p wpa` zakłada grupę bezpośrednio przez gniazdo wpa_supplicanta, z własnym dnsmasq.
Działa tylko bez NetworkManagera, bo inaczej NM usuwa grupę po około 80 ms.

## Przygotowanie płytki (Raspberry Pi OS trixie)

```bash
# /boot/firmware/config.txt: dekoder i KMS (z gpu_mem=16 firmware nie ma kodeków)
gpu_mem=128
dtoverlay=vc4-kms-v3d
# pakiety
sudo apt install --no-install-recommends gstreamer1.0-tools gstreamer1.0-plugins-good \
  gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly gstreamer1.0-alsa iw v4l-utils dnsmasq-base
```

Sprawdzenie: `vcgencmd codec_enabled H264` → `enabled`, jest `/dev/video10` (`bcm2835-codec-decode`),
w `iw list` są tryby `P2P-GO` i `P2P-client`, a `nmcli device` pokazuje `p2p-dev-wlan0`.

## Uruchomienie

```bash
sudo systemd-run --unit=wfd-sink /usr/bin/python3 /path/to/wfd_sink.py --name <nazwa-odbiornika>
journalctl -u wfd-sink -f
```

Opcje: `--source MAC` (przyjmuj tylko to źródło), `--no-audio`, `--rtp-port 19000`.
Po stronie laptopa odbiornik widać w `tools/miracast-list.py`, a łączy się z nim zwykły `wfd-source --sink <MAC>`.

## Pomiary (Pi Zero W, 2026-11-11)

| Klip (H.264 High, TS) | kmssink, obraz + dźwięk | CPU |
|---|---|---|
| 720p 5 Mbit/s | 30 kl./s, 0 zgubionych | ok. 50% |
| 1080p 8 Mbit/s | 30 kl./s, 0 zgubionych | ok. 50% |

Pierwsza sesja ze źródłem z repo: 1080p30, 12 Mbit/s, LPCM, około 70 s, zakończona przez TEARDOWN źródła.

## Znane ograniczenia

- Nie ma jeszcze pomiaru opóźnienia ani długiej sesji.
- Bez HDCP, UIBC i trybu przez sieć szkolną (MS-MICE).
- brcmfmac pozwala na jeden interfejs grupy; osierocone `p2p-wlan0-N` odbiornik usuwa przy starcie i po sesji.

Testy logiki: `python3 -m unittest receiver/test_wfd_sink.py`.
