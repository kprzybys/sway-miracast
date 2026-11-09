# Samsung HG50BU800 (Tizen 6.5): oprogramowanie odbiornika Miracast, wyniki researchu (2026-11-09)

## 1–4. Źródła i wcześniejsze rozpoznanie

- Samsung Open Source Release Center (opensource.samsung.com) wymaga przeglądarki (klienci automatyczni dostają 403).
  Paczkę dla platformy tego TV, `22_DTV_NikeL`, pobrano ręcznie przeglądarką; jej zawartość opisuje §5.
- Publicznej dokumentacji parametrów `wfd_sec_*`, bufora odtwarzacza Smart View ani ograniczeń firmware B2B dla
  mirroringu nie znaleziono. Stos multimedialny Tizena jest oparty o GStreamer (prezentacje Tizen Developer Summit),
  co zgadza się z SDES TOOL `GStreamer` w RTCP z TV (`data/probe/capabilities.md` §6).
- Otwarte implementacje i specyfikacje użyte jako punkt odniesienia: 01org/wds
  (https://github.com/01org/wds), iwd `test/wfd-source`, Microsoft MS-WFDPE
  (https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-wfdpe/584aadfe-f0db-4dec-9612-6a1d7bae1b77),
  wzorcowy odbiornik Tizena libmm-wfd (`docs/tizen-sink-research.md`).

## 5. Paczka `22_DTV_NikeL` — co w niej jest (przeczytane 2026-11-09)

Źródło: Samsung Open Source Release Center, paczka 22_DTV_NikeL (sterowniki jądra na GPL-2.0); cytowane są tylko
nazwy symboli i krótkie fragmenty komentarzy.

Pobrana ręcznie przeglądarką: `22_DTV_NikeL.zip` (838 MB, 106 archiwów). Platforma zgodna z TV
(`22_NIKEL_UB1_B2B`). Kopie rozpakowano tylko do katalogu tymczasowego, nie do repo.

**Brak:** player/sink Miracast (libmm-wfd, wfd-manager), wpa_supplicant, wifi-direct-manager. Logika odbiornika WFD
jest w zamkniętym userspace i nie została opublikowana. GStreamer, gst-rtsp-server, pulseaudio, alsa-lib to zwykłe kopie OSS.

**Jest i jest istotne:** `tztv-media-nikel.zip`, czyli sterowniki jądra multimediów SoC (`sdp_*_nike`).

### Dekoder obrazu (MFC, `sdp_codec_nike/sdp_mfc.c`)
- Sprzętowy tryb **FREERUN** na instancję dekodera (`SDP_MFC_FREERUN` 0x84, bity inst0/inst1), ustawiany z userspace
  przez `V4L2_CID_SET_FREERUN`. W freerun dekoder taktuje wyświetlanie zadanym framerate'em
  (`mfcext_set_dec_freerun` → `SDP_MFC_DECSYNC_FRAMERATE`), a nie porównaniem PTS z STC.
  → **Zgodne z Q6 (TV ignoruje PTS obrazu).** Kod nie mówi, czy player Miracast włącza freerun (zamknięty userspace),
  ale to najprostsze wyjaśnienie obserwacji. Wniosek, nie dowód.
- Gdy freerun jest wyłączony, obraz synchronizuje się do STC ze stałym przesunięciem `av_sync_offset`
  (`SDP_MFC_STC_OFFSET`), na sztywno `STC_OFFSET_DVB = -9778` (komentarz „-120ms-64ms”, RQ171107-00538).
  W tickach 90 kHz to −108,6 ms. Jednostka nie jest pewna, bo komentarz nie zgadza się z tą liczbą.
  Wybór ATSC/DVB jest zakomentowany, więc zawsze DVB.

### Dźwięk (`sdp_alsa_nike`)
- DSP audio ma własną maszynę AV-sync do STC: `rAE_AV_SYNC_EN_MAIN/SUB/TRI`, stany `AVS_SKIP/REPEAT/LOCK/UNLOCK`,
  `lldAe_MediaOutAvsyncTargetPtsSet`, `rAE_FREERUN_MODE_AVSYNC`. Dźwięk więc **respektuje PTS** (skip/repeat do STC).
  → Zgodne z naszym obrazem: dźwięk trzyma PTS, obraz nie.
- `AUDIO_OUT_MIRACAST_BLOCK_SIZE = 80 próbek × 2 kan × 2 B × 6 AU + 4`, opisane jako „clone LPCM”. To bufor
  **wyjścia** Miracast (TV jako źródło / klon dźwięku), nie odbiornika.
- Pozycje fabryczne `ID_AV_AUDIO_DELAY_MODE`, `ID_SOUND_LIPSYNC_*`, `ID_GAME_MODE_DELAY_50/60`, `ID_AV_FUNC_MIRACAST`
  to tylko numery w enumie, bez logiki.

### Inne potencjalnie przydatne (nie czytane)
- `mt7921.zip`, `mt7668.tgz`: sterowniki Wi-Fi MediaTek. Który moduł jest w TV, nie sprawdzono. Może wyjaśnić
  zachowanie kanału P2P / MCC (zob. przycięcia przy roamingu STA).
- `linux.tgz` (jądro), `connman.zip`.

### Co z tego wynika dla A/V
- Model „obraz freerun + stały bufor, dźwięk do STC/PTS” potwierdza wniosek z pomiarów: offset to różnica buforów.
  Zegarem tego nie zgramy. Jedyna dźwignia po naszej stronie to przesunięcie PTS dźwięku (AUDIO_DELAY_MS) albo opóźnienie obrazu.

### Tryb niskiego opóźnienia: co mówi kod (2026-11-09, po teście `wfd_sec_low_latency_mode: on`)
- Kodu, który czyta `wfd_sec_low_latency_mode`, **nie ma** ani w `22_DTV_NikeL` (przeszukane: sterowniki `sdp_*`,
  GStreamer, gst-rtsp-server, pulseaudio, connman, mt7668/mt7921), ani w publicznym libmm-wfd (tizen-sink-research.md).
  Zajmuje się nim zamknięty userspace odbiornika.
- Dźwignie w sterownikach, z których taki tryb może korzystać (istnieją, ale nie wiadomo, czy ten tryb ich używa):
  - obraz: `sdp_drm_nike/dp/nikel/src/sdp_dp_common.c` `__dp_comm_set_video_inputlag_mode`: DEFAULT / LOWEST /
    USERDEFINE ustawia `inputlag_mode` i `mm_gamemode_flag` (game mode ścieżki wyświetlania dla dekodera, rejestr
    `REG_TMG_A_GAME_MODE` w `sdp_dp_layer.c`);
  - dźwięk: `sdp_alsa_nike/src/sdp_aio_dma_hal.c` `i2sTx1StcIntSizeForGameMode`: przerwanie DMA co 5,33 ms zamiast
    32 ms, gdy włączony Game Mode Boost i nie ma pilota zdalnego (`!isRemoteConnected()`); różnica rzędu 27 ms;
  - wzorzec Tizen (libmm-wfd): `ts-offset` audio 250 ms, jitterbuffer 10 ms, `buffer-time` 100 ms — najwięcej
    milisekund do zabrania.
- Rozróżni to pomiar: zmiana opóźnienia dźwięku ~25 ms wskazywałaby na DMA, ~100 ms i więcej na bufor playera (ts-offset).
