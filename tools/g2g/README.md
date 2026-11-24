# tools/g2g: pomiar opóźnienia od ekranu do ekranu (glass-to-glass)

Narzędzia z 2026-11-11, użyte do pomiarów odbiornika `receiver/` na Pi Zero W. Ekran laptopa nie miga:
kod czasu rysuje się na wirtualnym wyjściu Sway (`swaymsg create_output`), które `wfd-source` nadaje zamiast `eDP-1`.

| Plik | Do czego |
|---|---|
| `stripes.py` | generator w kitty: kalibracja (14 pasów na przemian i odwrotnie, stała średnia jasność), potem 14 pasów: biel, kod Graya 12 bit licznika klatek, czerń; dziennik „licznik, czas” |
| `g2g_code.sh` | sesja na wirtualnym wyjściu + kamera USB (`CAM`, domyślnie `/dev/video2`) patrzy na ekran odbiornika |
| `g2g_code.py` | analiza nagrania: dopasowanie wzoru 14 równych pasów, odczyt licznika z każdej klatki kamery |
| `g2g_flash.py` | analiza starej metody z błyskami (generator `screenflash.py` jest tylko w lokalnym, nieśledzonym `tools/archive/`) |
| `srcprobe.sh` + `sink_sim_ts.py` | samo źródło: `wfd-source --no-p2p` do lokalnego symulatora odbiornika z czasem przyjścia pakietów |
| `genlag.sh` | czy generator nadąża: zrzuty `grim` wirtualnego wyjścia kontra dziennik generatora (opcjonalnie w trakcie nadawania) |
| `lanprobe.sh` + `sink_ctl.py` | to samo źródło i potok odbiornika, ale przez zwykłą sieć zamiast P2P (test A/B łącza) |
| `owd.py` | względne opóźnienie pakietów RTP na odbiorniku (AF_PACKET, root): narastanie kolejki po drodze |
| `stageprobe.py`, `digiprobe.py` | sondy GStreamera na odbiorniku (czasy etapów, licznik z pikseli za dekoderem). Uwaga: Python na Zero W sam spowalnia potok |

## Pułapki (sprawdzone)

- **`ffmpeg` do rawvideo dubluje klatki** przy nieregularnych znacznikach czasu kamery USB (1507 → 2302 klatek).
  Indeks klatki rozjeżdża się wtedy z `pts` i wychodzi fałszywe, rosnące opóźnienie. Zawsze `-fps_mode passthrough`.
- Kamera ma automatyczną ekspozycję: kalibracja „cały biały / cały czarny” się spłaszcza, dlatego pasy na przemian.
- Kitty z drobną czcionką nie nadąża rysować 30 klatek/s (zaległość w terminalu): czcionka 20, sprawdzenie `genlag.sh`.
- Kalibracja dopiero po przejściu okna na pełny ekran (generator czeka 3 s).
- Wynik zawiera nieskalibrowane opóźnienie kamery (stałe, rzędu dziesiątek ms).

## Wyniki (Pi Zero W, `receiver/`, 2026-11-11)

720p30, 2 Mbit/s, obraz zmieniający się w każdej klatce: P2P około 0,40 s, zwykła sieć około 0,54 s, stałe.
Samo źródło (ekran → pakiet): 20–90 ms. Tabela dla innych przepływności: `receiver/README.md`.
