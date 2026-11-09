# Notatki ze specyfikacji: P2P v1.5, WFD v2.1, MS-WFDPE

Cytaty ze specyfikacji © Wi-Fi Alliance — krótkie, w celu komentarza; MS-WFDPE © Microsoft (Open Specifications).

Źródła (lokalnie w `ref/`, poza repo): Wi-Fi P2P Technical Specification **v1.5** (2014), Wi-Fi Display Technical Specification **v2.1**, MS-WFDPE (v20260810).
Skróty: `P2P §x` = spec P2P v1.5, `WFD §x` = spec WFD v2.1, `PE §x` = MS-WFDPE.
Cytaty są krótkie, z oryginału (angielski). „brak w spec." = nie znaleziono w tych trzech dokumentach.
Oznaczenia: **[MUST]** = „shall" w spec; **[SHOULD]** = „should"/zalecenie; **[MAY]** = opcjonalne; **[WNIOSEK]** = nasz domysł.
Uwaga ogólna: ani P2P, ani WFD nie definiują żadnego czasu, jak długo odbiornik ma być widoczny po „wejściu w Smart View".
To jest zachowanie UI producenta (Samsung), poza spec.

---

## 1. P2P discovery

### 1.1 Stany i kanały
- **Listen State** (P2P §3.1.2.1.1): urządzenie spoza grupy „may use the Listen State to become discoverable".
  Dwell na jednym *Listen Channel* wybranym spośród *Social Channels*: „Channels 1, 6, and 11 in the 2.4 GHz band shall be used as the Social Channels."
  **[MUST]** Listen Channel wybierany na początku In-band Device Discovery i „shall remain the same until P2P Discovery completes".
- Odpowiedzi w Listen: tylko na Probe Request z P2P IE, P2P Wildcard SSID, Wildcard BSSID i DA = broadcast lub własny P2P Device Address.
  Filtry: Requested Device Type (WSC IE) oraz P2P Device ID — odpowiedź tylko przy zgodności. SA i BSSID = P2P Device Address, SSID = „DIRECT-" wildcard, ESS=0, IBSS=0.
- **Kiedy urządzenie w ogóle odpowiada na Probe Request** (§3.1.2.1): „shall not respond to Probe Request frames unless it is: a P2P Group Owner or in the Listen State, or a P2P Device associated with an infrastructure AP on the channel on which the Probe Request was sent…" (ten ostatni przypadek: „may respond provided it is not already a member of a P2P Group").
  => Odbiornik poza Listen (i poza grupą/AP na tym kanale) jest niewidoczny. To jedyny mechanizm „widoczności" w spec.
- **Scan Phase** (§3.1.2.1.2): zwykły skan 802.11 po wszystkich kanałach; „The P2P Device in the Scan Phase shall not reply to Probe Request frames." Służy też do wyboru kanału operacyjnego.
- **Search State** (§3.1.2.1.3): Probe Request na każdym z kanałów Social (jeden lub więcej na kanał); wymagane: P2P IE, WSC IE (Device Name, Primary Device Type, Device Password ID), SSID = P2P Wildcard, BSSID = wildcard.
  „A P2P Device in the Search State shall not reply to Probe Request frames."
  PBC: gdy w aktywnym PBC (okno 120 s „walk time") Device Password ID = PushButton.

### 1.2 Find Phase – czasy
- **Find Phase** (§3.1.2.1.3): „shall alternate between the Listen and Search states". Zbieżność dwóch szukających urządzeń zapewnia losowość czasu Listen.
- **[MUST]** Czas każdego Listen w Find = losowa całkowita wielokrotność 100 TU, „no greater than the maxDiscoverableInterval value and no less than the minDiscoverableInterval", **domyślnie min=1, max=3** => 100/200/300 TU (1 TU = 1,024 ms, czyli ok. 102–307 ms). Przez ten czas urządzenie jest „constantly available on the Listen Channel".
- Appendix A (maszyna stanów): Listen -> (dwell minąć) -> Search; po przeskanowaniu wszystkich kanałów Social: „may transition back to the Listen state". Brak górnego limitu czasu Find Phase w spec.
- Poza Find Phase (§3.1.2.1.1): „may stay in the Listen State for an extended period of time." Zalecenie **[SHOULD]**: „available in the Listen State for at least a contiguous period of 500ms every 5s" — to jest jedyna liczba o „jak długo widoczny" w trybie niezwiązanym z Find. Przerwy (skan, power save) „may result in lengthened or unreliable discovery".
- **Extended Listen Timing** (P2P §4.1.10, attr 8): Availability Period (min. ciągła dostępność, ms) i Availability Interval (odstęp początków, ms); 65535/65535 = stale dostępny. Może być w Probe Req/Resp, GO Negotiation Request, (Re)Assoc Response (a w (Re)Assoc Request = „timing desired of a Persistent P2P Group Owner").
  §3.2.5: dla grup trwałych z bitem Persistent Reconnect okres dostępności „should be at least 10 milliseconds", a interwał „determines the typical time it shall take to discover the P2P Device" — dobierany do potrzeb aplikacji.
- Od czego zależy czas widoczności (podsumowanie spec): od tego, czy urządzenie jest w Listen (Find: 100–300 TU naprzemiennie z Search; poza Find: dowolnie długo, zalecane ≥500 ms/5 s lub deklarowane ELT), oraz od przerw na inne czynności. Decyzję „kiedy wejść/wyjść z Listen" daje UI/higher layer: Appendix A „some other (e.g. higher layer or user level) action terminates the Listen State".
- Szukający powinien Listen Channel celu znać z Probe Request/Response (§3.1.4.1): przy rozpoczęciu GO Negotiation wystarczy skan Listen Channel celu, nie wszystkich Social.
  „A P2P Device may start Group Owner Negotiation … when receiving a Probe Request frame from the target P2P Device." => źródło, które tylko skanuje i nie jest w Listen, traci okazję, gdy TV samo wyśle Probe Request/GO Neg Request.

### 1.3 Device Discoverability (GO Discoverability Request)
- P2P §3.1.2.2 / §3.2.4 / §4.2.9.7-8 / §4.2.10.5. Dotyczy **wyłącznie P2P Client w istniejącej grupie** (nie urządzenia poza grupą): szukający wysyła *Device Discoverability Request* (P2P Device ID + P2P Group ID) do GO; GO wysyła *GO Discoverability Request* do klienta; klient „should stay constantly available for at least a 100 TU after receiving a GO Discoverability Request frame or until communication with the searching P2P Device completes". Odpowiedź *Device Discoverability Response* (Status).
- Klient deklaruje obsługę bitem *P2P Client Discoverability* w P2P Capability w Association Request. Jeśli 0 — nie wolno do niego słać żadnych ramek (§3.2.4).
- [WNIOSEK] Dla TV niebędącego w żadnej grupie ta procedura **nie ma zastosowania**; nie da się „obudzić" TV poza Smart View tym mechanizmem. Jedyna rzecz po stronie źródła, gdy my jesteśmy GO a TV klientem: GO może użyć GO Discoverability w drugą stronę tylko wobec klienta.

### 1.4 Presence / Notice of Absence (istotne przy współbieżności)
- P2P §3.1.2.4: GO „stays on the Operating Channel and waits for other devices to discover it"; jeśli niedostępny na kanale operacyjnym, **[MUST]** „shall indicate this using the Notice of Absence mechanism" (§3.3.3.2).
- P2P §4.1.14: NoA sygnalizuje nieobecność z powodu power save, **concurrent operation** lub skanu off-channel. Maks. dwa równoległe harmonogramy NoA. Uwaga o współbieżności: parametry NoA „should be chosen… to balance the needs of both the WLAN and P2P Group".
- P2P §3.3.2: CTWindow (okno obecności GO od TBTT) dla GO chcącego być widocznym „should be at least 10 TU".
- Jeśli kanał P2P = kanał STA (single channel concurrency), NoA nie jest potrzebny do przełączania kanałów; samo przełączenie kanału P2P wymaga ECSA (§3.2.2).

---

## 2. Group formation

### 2.1 Ogólne (P2P §3.1.4.1)
- Formation = GO Negotiation + Provisioning. **[MUST]** „A P2P Device shall take no more than fifteen seconds to complete Group Formation." Dane do provisioning (PIN) muszą być znane przed formacją, bo WSC bywa do 2 min.
- **[MUST]** Jedna formacja naraz; ktoś w formacji, kto dostanie GO Neg Request od innego urządzenia, odpowiada GO Neg Response z błędem (Status 5, „unable to accommodate request"; §3.1.4.2.2).
- Przed formacją urządzenia muszą być na wspólnym kanale; po starcie GO Negotiation „shall stay on that channel until Group Owner Negotiation completes".
- Provision Discovery (PD): **[MAY]** PD Request z jedną metodą (Config Methods) przed formacją, by wywołać akcję u odbiorcy (np. wyświetlenie PIN). PD Request ma **dokładnie jedną** metodę (PBC = PushButton; Keypad/Display dla PIN; tabela 1). PD Response: ta sama metoda = sukces, null = porażka. Przy dołączaniu do istniejącej grupy bez poświadczeń PD Request jest **[MUST]** wysłany do GO na kanale grupy; brak PD Response w 100 ms = porażka, ale klient „may proceed regardless" (§3.2.3).

### 2.2 GO Negotiation (§3.1.4.2, trójstronna wymiana Request / Response / Confirmation)
- **Timeout**: nadawca uznaje negocjację za nieudaną i zakończoną, jeśli nie dostanie kolejnej ramki „within 100 milliseconds of receiving an acknowledgment frame". Każda ze stron może zacząć ponownie później. [WNIOSEK] 100 ms liczone od ACK oznacza, że odbiorca musi być na kanale i gotowy; wyjście TV z Listen na inny kanał zrywa formację.
- **Intent** (0–15): „If the P2P Device must be the P2P Group Owner, the Intent field … shall be set to 15." Zalecenie: 15 tylko gdy usługa działa wyłącznie na GO (np. cross connection). Remis (równe <15): wygrywa nadawca z Tie breaker=1. Tie breaker w pierwszym Request losowy, w kolejnych (poza retransmisjami) przełączany; w Response przełączony względem Request. 15 vs 15: Status 9 (§4.1.2, tabela 8).
- Dwa urządzenia wysyłają Request jednocześnie: odpowiada tylko to z wyższym P2P Device Address (§3.1.4.2.2).
- Request zawiera (**[MUST]**): P2P Capability, P2P Device Info, Channel List, Listen Channel, Operating Channel, GO Intent, Configuration Timeout, Intended P2P Interface Address + WSC Device Password ID (§3.1.4.2.1). Response: bez Listen Channel, z Status (§3.1.4.2.2). Device Password ID: PushButton dla PBC.
- Statusy błędu (P2P §4.1.2, tab. 8): 1 info unavailable, 2 incompatible parameters, 3 limit reached, 4 invalid parameters, 5 unable to accommodate, 6 previous protocol error/disruptive, 7 no common channels, 8 unknown group, 9 both intent 15, 10 incompatible provisioning method, 11 rejected by user, 12 success: accepted by user.
- Status 1 („Fail; information is currently unavailable"): odbiorca bez danych provisioning; jeśli w ciągu **120 s** pojawi się wejście użytkownika, ma sam zacząć GO Neg Request; inaczej nadawca „can assume that there will be no attempt to restart Group Formation".
  [WNIOSEK] Tak zachowuje się odbiornik, którego UI nie czeka na połączenie (nie jest w Smart View): odpowie Status 1 i nic więcej nie zrobi.
- **Wybór kanału operacyjnego**: GO (ten, który wygrał) podaje Channel List ⊆ listy z Request i Operating Channel ∈ Channel List; klient może podać preferowany kanał lub pominąć (§3.1.4.2.2-3). Brak wspólnych kanałów => Status 7 w Confirmation (§3.1.4.2.3).
  Reguły dla Managed/koegzystencji: §3.4.2 (patrz sekcja 4). Podczas provisioningu GO „shall use the Operating Channel indicated during Group Owner Negotiation, if available"; w przeciwnym razie inny kanał z Channel List z Confirmation, a klient „may have to scan to find the P2P Group Owner" (§3.1.4.3).
- Confirmation (§3.1.4.2.3): Status Success lub błąd (brak wspólnych kanałów, niezgodne parametry, niezgodna metoda). Wynik: pełna lista kanałów i grupa (SSID DIRECT-xy…, persistent bit itd.).
- **Concurrent z STA na jednym kanale 2.4 GHz** [WNIOSEK z §2.3, §3.1.4.2.2, §3.4.2]: spec pozwala, by grupa P2P działała „in the same or different operating class and channel as a concurrently operating WLAN BSS" (§2.3). Jako GO nasz Channel List w Response/Confirmation i Operating Channel powinny zawierać wyłącznie kanał, na którym stoi STA (inaczej GO przeskoczy na inny kanał i STA straci obecność). Jeśli TV jako klient nie obsługuje tego kanału: Status 7.
- Grupa jako GO bez negocjacji: §3.2.2 „The P2P Group Owner may be set by configuration" (autonomous GO). Wtedy brak GO Neg; formacja = PD (opcjonalnie) + WSC.

### 2.3 Provisioning / WSC (§3.1.4.3)
- GO startuje grupę (§3.2.2) z poświadczeniami; klient łączy się, by je pobrać. GO = AP z Internal Registrar, klient = STA Enrollee.
- **[MUST]** GO „shall only allow association by the P2P Device that it is currently in Group Formation with". Rejestrator „shall send M2 in response to M1 and shall not send M2D" (PIN/przycisk już użyty po obu stronach).
- **[MUST]** Bit **Group Formation** w P2P Capability (Beacon/Probe Response GO) = 1 do końca provisioning. Klient „shall not attempt to connect to a P2P Group that has the Group Formation bit … set to 1 unless it is in Group Formation with that P2P Device" (§3.2.3).
- Po sukcesie provisioning GO ustawia Group Formation = 0 i „At this point the P2P Client may join the P2P Group using the Credentials supplied during Provisioning" (§3.1.4.3). Tzn. po WPS klient robi **drugie, nowe** skojarzenie WPA2-PSK (4-way handshake) z poświadczeniami. Spec nie podaje na to osobnego limitu poza ogólnym 15 s dla całej formacji; Configuration Timeout (§4.1.7; jednostka 10 ms, 0–255, czyli maks. 2,55 s) to maksymalny czas, jaki urządzenie potrzebuje po zakończeniu GO Neg, żeby być gotowe do Provisioning (GO Configuration Time, Client Configuration Time); start provisioning można odroczyć do max z obu.
- Porażka provisioning: „If Provisioning fails then Group Formation ends and the P2P Group Owner shall end the P2P Group session as described in Section 3.2.9"; można ponowić formację lub wrócić do discovery. Appendix A: po porażce urządzenie może wejść w Scan, Listen, albo (GO trwałej grupy) w Listen („permanent").
- Zakończenie sesji (§3.2.9): GO wysyła Deauthentication (reason 3) na broadcast / do wszystkich klientów.
- WPS: spec P2P odsyła do Wi-Fi Simple Configuration (osobny dokument, nie mamy); PBC „walk time" 120 s (§3.1.2.1.2). Reguły WSC (M-messages, timeouty EAP) — brak w spec.

### 2.4 „FORMATION_FAILURE po WPS-REG-SUCCESS" – co z tego wynika
(Interpretacja; spec nie opisuje wprost logu wpa_supplicant.)
1. WPS-REG-SUCCESS po stronie GO = zakończony EAP-WSC (M8). Dalej **to klient** musi (a) wyjść z WSC, (b) znaleźć GO (na kanale operacyjnym; może skanować), (c) zrobić zwykłe skojarzenie WPA2 i 4-way. Jeśli TV nie wróci, GO nie ma co zrobić poza timeoutem (spec: brak osobnego timera GO na to; obowiązuje ogólne 15 s „no more than fifteen seconds to complete Group Formation" — [WNIOSEK] możliwe źródło FORMATION_FAILURE w wpa_supplicant, które ma własny timeout).
2. Sprawdzić, czy GO po WPS naprawdę przestał ogłaszać Group Formation=1 (§3.1.4.3, §3.2.3) — klient wg spec nie wolno łączyć się z grupą z bitem 1. To da się sprawdzić w Beacon/Probe Response (P2P Capability, Group Capability Bitmap bit 6).
3. Kanał: jeśli GO zmienił kanał między GO Neg a startem grupy (np. bo STA przeskoczył), klient szuka GO i może nie znaleźć (§3.1.4.3). Przy single channel concurrency kanał musi zgadzać się z kanałem STA.
4. Czas Provisioning przy odbiorniku, który już opuścił Listen/UI (np. Smart View skończył limit): spec nie ma tu nic (**brak w spec.**).
5. WFD §6.5.2: po udanym 4-way handshake 90 s na zestawienie TCP (patrz 5.2). To jest następny timer po FORMATION; TV, który nie robi 4-way, nigdy tam nie dojdzie.

---

## 3. Invitation Procedure i grupy trwałe (Persistent)

### 3.1 Co mówi spec
- **Invitation Request/Response** (P2P §3.1.5.1-3.1.5.3, §4.2.9.5-6, attr Invitation Flags §4.1.20): ramki P2P Public Action. Bit 0 *Invitation Type*: 1 = ponowne uruchomienie grupy trwałej, 0 = zaproszenie do aktywnej grupy.
- Request od GO zawiera (**[MUST]**): P2P Group ID, P2P Group BSSID, Channel List, Operating Channel, Configuration Timeout (GO Config Timeout = 0 przy zaproszeniu do aktywnej grupy; przy reinvoke: czas GO na start sesji). Od klienta (reinvoke grupy, w której jest klientem): Group ID, Channel List, Configuration Timeout; Operating Channel opcjonalny.
- Timeout: „shall assume that process failed if it does not receive a P2P Invitation Response frame within 100 milliseconds of issuing the P2P Invitation Request frame."
- Response: Status=0 + (od GO trwałej grupy) Group BSSID, Channel List (⊆ Request), Operating Channel, Configuration Timeout. Błędy: 7 (no common channels), 8 („Fail: unknown P2P Group" dla nieznanego Group ID przy Type=1), 1 (info unavailable — przekazano wyżej; odbiorca „shall not wait for any action from a higher layer", a po autoryzacji w ≤120 s sam wysyła nowy Invitation Request).
  „The decision to accept or deny the P2P Invitation Request frame is left to P2P Device's implementation policy."
- Reinvoke (§3.1.5.3, §3.2.5): GO trwałej grupy „may invoke a Persistent P2P Group at any time by starting a P2P Group session" — zaproszenie jest opcjonalne („Optionally, it may choose to only start … after a successful P2P Invitation Request and Response"). **[MUST]** Klient, który chce reinvoke, „shall first discover the P2P Device that is the P2P Group Owner … and then successfully complete a P2P Invitation exchange". GO „shall restart the Persistent P2P Group state on completion of a successful P2P Invitation exchange".
  Przy reinvoke Config Methods w P2P Device Info są ignorowane (nie ma PD/WSC). Po wymianie klient szuka GO na Operating Channel, a jeśli go nie ma — skanuje kanały z Channel List (GO mógł wybrać inny kanał).
- Trwałość: poświadczenia i Group ID stałe; BSSID, adresy interfejsów i kanał **mogą się zmienić** między sesjami (§3.2.5). Bity Persistent P2P Group / Persistent Reconnect (Group Capability bity 1 i 5; §4.1.4) stałe między sesjami. Persistent Reconnect = 1 oznacza łączenie „without user intervention" i dopuszcza inny timing Listen (ELT).
  Grupa trwała kończy się, gdy GO skasuje poświadczenia.
- Wymóg widoczności: Appendix A: „If a P2P Device that is a member of a persistent group is in the listen phase, it may receive a P2P Invitation Request from a previously provisioned P2P Client or Group Owner." Zaproszenie wysyła urządzenie, które **już odkryło** adresata (§3.1.2.2-3.1.2.3: po discovery „may send a P2P Invitation Request"). Odkrycie wymaga Listen u adresata. Czy samo Invitation można wysłać bez wcześniejszego Probe — **brak w spec.** (ramka to Public Action na kanale adresata; wymóg obecności adresata na kanale wynika z natury ramki, nie z tekstu).
- Wymóg WFD (WFD §4.13): „A Persistent WFD Group … eases the process of WFD Device Discovery and WFD Connection establishment"; dla P2P „required information to be stored is the P2P Group ID and Credentials"; „No additional Wi-Fi Display specific information is cached" (§4.13.1). R2 „shall be capable" używać grup trwałych P2P. Dla R1/R2 w Table 3 persistent group = **O** (opcjonalne) dla Source i Sinków. WFD IE ma być w Invitation Request/Response (WFD §4.5.2.1); niezgodny Device Type nadawcy -> Status 2.

### 3.2 Co to znaczy dla TV poza Smart View
- [WNIOSEK z spec] Invitation nie omija warunku odkrywalności: TV musi być w Listen (lub być GO/klientem w grupie), żeby w ogóle odebrać ramkę i odpowiedzieć w 100 ms. Spec nie wymaga, by TV bez aktywnego UI był w Listen; „Fail: information is currently unavailable" (Status 1) to przewidziana odpowiedź „przekazane do wyższej warstwy".
- Invitation do trwałej grupy z TV jako klientem wymaga, żeby **TV zapamiętał grupę** (Group ID + poświadczenia) i obsługiwał *P2P Invitation Procedure* (bit w P2P Capability, Device Capability bit 5). Czy Samsung to robi — brak w spec. (do sprawdzenia empirycznie: bit w Probe Response TV; `P2P-DEVICE-FOUND ... dev_capab=0x..`, bit 0x20).
- Po zdalnym reconnect TV nadal potrzebuje WFD: TCP z TV do źródła (patrz 5.2) i pełnego M1–M7. Spec nie gwarantuje, że TV pokaże UI Smart View po cichym reconnect (**brak w spec.**).
- [WNIOSEK] Jeśli TV nie odpowiada na Invitation poza Smart View, nie ma rozwiązania wyłącznie w spec; trwała grupa pomaga tylko pominąć WPS/PD, gdy TV jest już widoczny.

---

## 4. Praca współbieżna (P2P + infrastruktura WLAN)

- **Definicja** (P2P §2.3): P2P Concurrent Device ma wiele MAC (fizycznych lub wirtualnych); „A P2P Group may operate in the same or different operating class and channel as a concurrently operating WLAN BSS." Adres interfejsu P2P musi być inny niż adres STA (§2.4.3).
- **Bit Concurrent Operation** (Device Capability, bit 2, §4.1.4): ustawiony, gdy urządzenie wspiera współbieżność z WLAN.
- **Cross Connection** (Group Capability, bit 4): „hosting, or intends to host, a P2P Group that provides cross connection between the P2P Group and a WLAN". Ustawiamy 0, bo nie przekazujemy ruchu między grupą a AP sieci, do której podłączony jest laptop. Intent 15 jest zalecany dla cross connection (§3.1.4.2) — nas to nie dotyczy.
- **Managed P2P Device** (P2P §3.4, opcjonalne): AP „capable of managing P2P Devices" dodaje do P2P IE w Beacon/Probe Response/(Re)Assoc Response atrybut **P2P Manageability** (attr 10, §4.1.12) z bitami: B0 *P2P Device Management*, B1 *Cross Connection Permitted*, B2 *Coexistence Optional*. B0=0 znaczy „no desire to manage P2P Devices".
  Urządzenie zarządzane ustawia *P2P Infrastructure Managed* = 1 w P2P Capability w Probe/(Re)Assoc Request do AP.
- **AP może rozłączyć STA** (P2P §3.4.3) — z P2P IE zawierającym atrybut **Minor Reason Code** (§4.1.3, tab. 10):
  - 1: cross connection poza polityką IT (niezależnie od Infrastructure Managed);
  - 2: STA ma Infrastructure Managed = 0 (urządzenie niezarządzane, a AP je zarządza);
  - 3: współbieżny P2P niezgodny z Coexistence Parameters;
  - 4: STA zgłosił Infrastructure Managed = 1, a P2P jest poza polityką.
  Wszystkie są **[MAY]** po stronie AP. Zalecenie dla klienta („advisable"): nie reasocjować do AP, dopóki warunki się nie zmienią, albo „The P2P Device may attempt to re-associate … with P2P Device capability disabled. Disabling P2P Device capability includes omitting the P2P IE from all Management frames, leaving all P2P Groups and not responding to Invitation Requests."
- **Coexistence Parameters** (§3.4.2): Primary (informacje o kanałach; „Managed P2P Device that is a P2P Concurrent Device should use Primary P2P Coexistence Parameters as part of channel selection") i Secondary (moc, WMM). Jeśli AP ogłasza *Coexistence Optional = 0*, współbieżne urządzenie zarządzane „shall adopt" parametry (Table 2/§3.4.2), „The Enterprise IT department may not allow Concurrent P2P Devices to connect to the WLAN infrastructure if these devices do not adhere".
- Tabela 3 (§3.4.4): niezarządzane GO współbieżne (nasz przypadek: Infrastructure Managed = 0) musi „Understands Minor Reason Code set to 1 or 2" i nie włączać cross connection przy AP z Cross Connection Permitted = 0; alternatywnie „can disable P2P Device capability before (Re)associating to the WLAN".
- Zalecenie WFD §4.14.1: „should support concurrent traffic access over the Wi-Fi BSS, with minimal disruption to the BSS and other devices on the Distribution System", i „shall conform to the procedures in [7]".
- **Czy AP sieci współdzielonej może rozłączać nasz STA przez P2P?** Spec: tak, ale **tylko** z Minor Reason Code w P2P IE ramki Deauth/Disassoc i tylko AP oferujący zarządzanie P2P (§3.4.1). Rozłączanie *bez* tych atrybutów (typowy AP/kontroler sieci publicznej: ochrona przed „rogue", band steering, client isolation, brak aktywności w czasie off-channel) jest poza spec P2P; zwykłe 802.11: STA, który zniknął na kanale na czas Find/Listen, może zostać uznany za nieaktywny (**brak w spec.** P2P; to 802.11 [1], którego nie mamy).
  Testy: (1) zdekodować Beacon/Probe Response tego AP i sprawdzić czy zawiera P2P IE (OUI 50:6F:9A typ 9) z P2P Manageability; (2) na Deauth/Disassoc od AP sprawdzić reason code 802.11 i obecność P2P IE z Minor Reason Code (tryb monitor/`iw event -t`, `wpa_supplicant -dd`); (3) sprawdzić, czy nasz Association Request zawiera P2P IE z bitem Cross Connection / Infrastructure Managed (domyślne ustawienia wpa_supplicant dla interfejsu STA). Brak P2P IE w Deauth = rozłączenie nie wynika z P2P Manageability.
- Jeden kanał STA/P2P: spec wprost dopuszcza ten sam kanał (§2.3). Trzymanie grupy P2P na kanale STA nie wymaga NoA/przełączania; skan Find na kanałach 1/6/11 wymaga opuszczenia kanału STA (power save null frame, krótkie nieobecności) — mechanizmy PS/NoA STA są w 802.11 (**brak w spec.**).

---

## 5. Warstwa sesji WFD

### 5.1 WFD IE i dostępność sesji
- WFD IE (WFD §5.1.1) w Beacon, Probe Request/Response, Assoc Req/Resp, GO Neg Req/Resp/Conf, Invitation Req/Resp, PD Req/Resp (§4.5.2.1). WFD IE niesie subelement 0 *WFD Device Information* (§5.1.2): 2 oktety bitmapy (Table 29), Session Management Control Port (domyślnie 7236), Max Throughput.
- Bity Table 29: [1:0] Device Type (00 Source, 01 Primary Sink, 10 Secondary Sink, 11 dual); [2],[3] coupled sink; **[5:4] WFD Session Availability** (00 not available, 01 available); [6] WSD; [7] PC (0=P2P, 1=TDLS); [8] CP/HDCP; [9] time sync; [10] audio unsupported at Primary Sink; [11] audio-only source; [12],[13] TDLS persistent.
- **[MUST]** (§4.5): „If a discovered WFD Device sets WFD Session Availability bits (B5B4) … to 0b00 (i.e., not available), other WFD devices shall not attempt WFD Connection establishment with that WFD Device until that WFD Device indicates its availability by setting same bits to 0b01".
  [WNIOSEK] Samsung może przełączać bity 5:4 (0b00 gdy UI nie czeka na źródło) — to zgodne ze spec i może tłumaczyć „krótkie okno": TV widoczny w Probe Response, ale „niedostępny". Dekodować `wfd_dev_info` z `P2P-DEVICE-FOUND`/Probe Response i sprawdzić bity 5:4 w czasie.
  Treść Device Information subelement „should be immutable during the period of P2P connection establishment", wyjątki: dual-role Device Type i właśnie Session Availability (§4.5.2.1).
- Nasz WFD IE jako Source: Device Type=00 (nie 11 — „In any other cases, these bits shall not be set to 0b11"); Session Availability=01 gdy gotowi; rola stała podczas zestawiania. Niezgodna rola w GO Neg/Assoc/Invitation => Status 2 (§4.5.2.1). Urządzenie, które chce być odkryte, ustawia bit *P2P Discoverability* = 1 w Device Capability P2P (§4.5.2; **[MUST]** „A WFD Device which intends to be discovered shall set the P2P Discoverability bit…").
- GO odpowiada na Probe Request z WFD IE Probe Response z WFD IE sesji (subelement 9, §5.1.10) listującym klientów WFD (§4.3.2, §4.4.2.2).
- Dla jednego sinka: „either a WFD Source or a WFD Sink may be a P2P GO" (§3.2.1); GO jest narzucony źródłu tylko przy Primary+Secondary Sink. Przy Coupled: sink prosi o GO Intent 0 lub źródło ustawia GO-only (§4.9.x).

### 5.2 Przebieg zestawiania i **wszystkie** terminy RTSP
Kolejność (§4.2, §4.6, §4.8, §6.4): L3 gotowe -> TCP (inicjuje **sink**, źródło = serwer TCP; port kontrolny 7236 lub 49152–65535, §4.5.4, §5.1.2) -> M1 (Source: OPTIONS) -> M2 (Sink: OPTIONS) -> M3 (Source: GET_PARAMETER) -> M4 (Source: SET_PARAMETER, z `wfd_presentation_url`) -> M5 (Source: trigger SETUP) -> M6 (Sink: SETUP, odpowiedź z `Session:`) -> M7 (Sink: PLAY). Po M7 = sesja WFD ustanowiona (§4.8, §6.4).
- **§6.5.2 przed RTSP**: przy P2P TCP musi powstać „within 90 seconds of a successful transmission of the M4 Message of the Four-Way WPA2 handshake"; źródło „shall start the RTSP server"; sink powtarza próby TCP w regularnych odstępach do 90 s. Po 90 s urządzenia „may teardown the Wi-Fi P2P or TDLS link". [WNIOSEK] Źródło musi mieć już nasłuchujący socket (port z WFD IE) **zanim** TV zrobi 4-way (w tym przy restarcie procesu).
- **§6.5 reguły (RTSP timeout)**:
  1. Po nawiązaniu TCP Source **[MUST]** wysyła M1 w ≤ **6 s**.
  2. Timeout wymiany żądanie-odpowiedź: **5 s** (poza M16).
  3. Przed ustanowieniem sesji: między odebraniem odpowiedzi a wysłaniem następnego żądania: **6 s** (np. M3 resp -> M4 req).
  4. Przed ustanowieniem sesji: między wysłaną odpowiedzią a następnym żądaniem: **6 s** (np. M2 resp -> M3 req).
  5. M6 resp -> M7 req: **6 s** (bez content protection) lub **9 s** (z HDCP).
  6. Po ustanowieniu sesji: M5 resp -> odpowiadające M7/M8/M9: **6 s**.
  7. Po ustanowieniu sesji: M4 resp -> M6 (przełączenie transportu): 6 s.
  „If any of the RTSP timeout rules is not satisfied, the RTSP procedures (and RTSP session if it has been established) shall be aborted, and a WFD Source and a WFD Sink returns to an RTSP Init state."
- **Błędy** (§6.4): zła składnia => odbiorca ignoruje wiadomość, przerywa procedury (i sesję). Kod błędu RTSP w odpowiedzi => urządzenie „shall not execute any behavior that is not accepted"; można ponowić z innymi parametrami albo przerwać. Przed M6: sink nie wysyła M8, a źródło nie wysyła M5 TEARDOWN; oba w stanie Init i „the WFD Source may send an RTSP M1 Request message to a WFD Sink". Po M6: sink wysyła M8, źródło M5 trigger TEARDOWN. Nieznane parametry: „A WFD Device should ignore any parameter not specified in Table 97" (§6.2) i „highly recommended that the WFD Sink should ignore an RTSP parameter that is unknown" (§6.4.4); w M3 responder „may ignore parameters in the request that the responder does not recognize" (§6.2.2).
- Wymagane URI: źródło używa `rtsp://localhost/wfd1.0` w żądaniach do sinka; sink używa `wfd-presentation-url` (§6.2).
- Pierwszy kadr po PLAY (§6.4.7): „the first video frame shall be an IDR picture with SPS and PPS"; nadawanie na porty RTP z M6 Request. Po wznowieniu zalecane PAT/PMT na początku TS (§4.15).
- Nieprawidłowe stany: M7 w stanie Playing -> 406 „in-play-state"; M9 w Ready -> 406 „in-pause-state" (§6.4.7, §6.4.9).

### 5.3 Keep-alive (M16) — §6.5.1, §6.4.16
- Źródło **[MUST]** wysyła M16 (GET_PARAMETER bez ciała). Timeout podaje źródło w nagłówku `Session:` w odpowiedzi M6 (`timeout=`); **[MUST]** „not … smaller than 10 seconds", domyślnie **60 s**, jeśli nie podano.
- Odstęp między kolejnymi M16 **[MUST]** < (timeout − 5 s). Przy timeout=60: <55 s. Przy 10: <5 s.
- Sink odpowiada na M16 (dowolny status liczy się jako „żywy"). Brak jakiejkolwiek odpowiedzi M16 w oknie timeout: Source „shall abort the RTSP session and RTP session"; sink bez M16 w oknie: sink przerywa sesję.
- Źródło w standby może wysyłać tylko M16 i M5 (PLAY/TEARDOWN) (§4.15).

### 5.4 wfd_idr_request (M13)
- WFD §4.10.5/§6.4.13/§6.1.20: sink wykrywa błąd strumienia i wysyła SET_PARAMETER z `wfd_idr_request`. **[MUST]**: „the WFD Source shall send an IDR picture with SPS and PPS in the bit stream and send a Response message with a status code of RTSP OK at the earliest opportunity." Odpowiedź M13 jest wymagana także przy braku obsługi (kod błędu, §6.4.13).
- PE §2.6.1.1: parametr `wfd_idr_request_capability` (0/1) — czy sink w ogóle wysyła M13; „A Wi-Fi Display Source that determines that it will never receive any RTSP M13 messages can insert IDR pictures more frequently".
- Zmiana formatu wideo (rozdzielczość/refresh) wymaga IDR zaraz po zmianie (§4.10.3.2). IDR potrzebny też przy starcie/wznowieniu (§6.4.7).
- [WNIOSEK] Stały wzorzec wfd_idr_request od TV oznacza stratę/zakłócenia w odbiorze (np. off-channel nieobecność źródła przy współbieżnym STA, gubienie pakietów) albo brak pierwszego IDR. Zbyt długi GOP bez IDR na żądanie jest niezgodny ze spec.
- Frame skipping (§4.10.3.1): źródło może pomijać klatki, jeśli sink to deklaruje (bit frame-rate-control) i nie przekroczy *Max Skip Interval* z M3; PCR co ≤ 100 ms; po przerwie SCR/DTS/PTS spójne. Powtarzanie ostatniej klatki (nasz tryb) zgodne z duchem spec, ale musi dawać ciągłość TS/PCR.

### 5.5 Standby / resume (§4.15, §6.4.12; opcjonalne)
- Wspierane tylko, jeśli M3 pokaże wfd-standby-resume-capability (Extended Capability bit 3). M12 `wfd_standby` — odbiorca odpowiada OK, po czym „shall" utrzymać sesję i odpowiadać na RTSP; źródło „should" przestać słać RTP i wyłączyć enkoder.
- Wznowienie: 4 przypadki (M5 PLAY od źródła -> M7 od sinka; lub M7 od sinka). Pierwszy PLAY po resume: najpierw PAT/PMT, potem IDR.
- Standby ≠ teardown: sesja RTSP/TCP trwa. W standby źródło odpowiada na inne żądania kodem 406 „in-standby-mode".
- [WNIOSEK] Jeśli TV porzuca ekran Smart View po przerwie w RTP, standby (M12) lub stały strumień (powtarzanie klatki + PCR) jest zgodną ze spec alternatywą dla restartu strumienia; to, co TV zrobi, spec nie definiuje.

### 5.6 TEARDOWN i zakończenie (§4.12, §6.4.8)
- Urządzenie kończące połączenie „should initiate a WFD Session termination using RTSP TEARDOWN" (M8). Sesja kończy się też bez TEARDOWN: timeout (§6.5) albo wyłączenie WFD przez użytkownika.
- M8 od sinka (**[MUST]**): źródło odpowiada; przy OK „shall abort the RTSP procedures, and shall stop the audio/video streaming and terminate the corresponding RTP session(s)". Nowa sesja po teardown: „shall start with WFD Capability Negotiation … and WFD Session establishment" (czyli od M1; nie wznawia się RTSP).
- Źródło może prosić o teardown triggerem M5 TEARDOWN (§6.4.5). Po teardown „a WFD Device may terminate a WFD Connection" (rozłączenie P2P, §4.12.1: „orderly termination … using the connection tear-down procedure defined in [7]", tj. P2P §3.2.7-3.2.9).
- Co TV robi z ekranem Smart View po TEARDOWN lub porażce formacji (wychodzi z Smart View) — **brak w spec.**; zachowanie producenta.
- Spec o zachowaniu źródła przy błędach: patrz 5.2 „Błędy" (abort -> Init -> źródło może znów M1). Brak wymogu, by źródło po błędzie rozłączało P2P; to opcjonalne (§4.12).

### 5.7 UIBC (krótko, §4.11)
Opcjonalny kanał zwrotny wejścia z sinka do źródła: pakiety (nagłówek: wersja, T, kategoria Generic/HIDC, długość, opcjonalny timestamp) po **jednym** połączeniu TCP przez cały czas sesji. Negocjacja M3/M4/M14 (`wfd_uibc_capability`, `wfd_uibc_setting`), port TCP źródła podany w `wfd_uibc_capability`; źródło musi nasłuchiwać przed wysłaniem M4/M14. Wymagany tylko, gdy obie strony deklarują (Extended Capability bit 0). Nie jest potrzebny do streamingu.

### 5.8 Parametry spoza spec
- `wfd_sec_view_mode` — **brak w spec.** (WFD v2.1, P2P v1.5 i MS-WFDPE; nie znaleziono tego ciągu w żadnym z dokumentów, ani w RFC w `ref/`). Samsung-vendor. Zasada ze spec: nieznany parametr należy zignorować i odpowiedzieć (§6.2, §6.4.4; M-odpowiedź jest wymagana na każde żądanie, §6.4). Zalecenie [WNIOSEK]: odpowiedzieć `200 OK` (lub kodem błędu, ale nie milczeć w 5 s — reguła 2 w §6.5 — bo brak odpowiedzi to naruszenie timeoutu i wg §6.5 sprowadza do Init).

---

## 6. MS-WFDPE (Microsoft Wi-Fi Display Protocol Extension)

Rozszerza WFD §6.1 o parametry RTSP; wszystkie opcjonalne, negocjowane w M3 (źródło pyta, sink odpowiada „supported"/„none"). Źródło musi do nich wymagać zgodności tylko, gdy sink je ogłosi.
- §2.1 Device Metadata: `intel_friendly_name`, `intel_sink_device_URL`, `intel_sink_manufacturer_logo` (PNG 160x120), `intel_sink_manufacturer_name`, `intel_sink_model_name`, `intel_sink_version` (product_ID/hw/sw) — **identyfikacja modelu sinka z M3** (przydatne do profilu w `sinks/`).
- §2.2 Enhanced Diagnostics: `microsoft_diagnostics_capability` i **`microsoft_teardown_reason`** w ciele M8: HRESULT + tekst. Predefiniowane kody: MF_E_CANNOT_PARSE_BYTESTREAM (C00D36F0), MF_E_INVALID_FORMAT (C00D3E8C), MF_E_TRANSFORM_CANNOT_CHANGE_MEDIATYPE_WHILE_PROCESSING (C00D6D74), MF_E_INVALID_STREAM_DATA (C00D36CB), **MF_E_NET_TIMEOUT (C00D4278: „timed out waiting for a keep-alive or RTP data")**, MF_E_INVALID_TIMESTAMP (C00D36C0). Kody własne mają bit 0x20000000. [WNIOSEK] Jeśli TV poda to w M8, pozwoli odróżnić „brak RTP/keepalive" od błędu strumienia (jeśli Samsung nie ogłasza capability — brak danych).
- §2.3 Dynamic Resolution/Refresh Rate: `microsoft_format_change_capability`: sink monitoruje SPS/PPS; zmiana rozdzielczości/fps bez M4 — wymagany IDR jako pierwsza klatka po zmianie i nowe SPS/PPS.
- **§2.4 Latency Management: `microsoft_latency_management_capability`**. Źródło pyta w M3 (nazwa bez wartości), sink odpowiada `supported`/`none`; źródło ustawia tryb przez **SET_PARAMETER** z wartością `low`/`normal`/`high` (przykład w PE §3: `SET_PARAMETER rtsp://localhost/wfd1.0 ... microsoft_latency_management_capability: low`).
  Definicja latencji: „the time from the moment the last RTP packet is received for a video frame, until the time that the Wi-Fi Display Sink renders the frame". Wartości: **low** = „SHOULD keep latency under 50 milliseconds"; **normal** = „under 100 milliseconds"; **high** = bufor dodatkowych klatek, latencja „under 500 milliseconds". Nie ma gwarancji (Windows 10 1507 > 100 ms nawet przy low/normal, przypisy <7>,<8>). Dotyczy tylko opóźnienia dekodera/renderingu sinka, nie sieci i enkodera źródła.
  Ustawienie `low`: dozwolone tylko po `supported` od sinka; zwykły WFD v2.1 ma osobne pole *decoder latency* w `wfd_video_formats` (WFD §6.1.3, wielokrotność 5 ms) — to deklaracja opóźnienia dekodera, nie sterowanie.
- §2.5 Source Identification: nagłówek `Server: MSMiracastSource/<ver> guid/<uuid>` w odpowiedziach źródła.
- §2.6 `wfd_idr_request_capability` (patrz 5.4).
- §2.7 Dodatkowe formaty: `wfdx_video_formats` (zastępuje `wfd_video_formats`, 4K, 3:2), `microsoft_video_formats`, `microsoft_custom_video_formats`.
- §2.8 RTCP: `microsoft_rtcp_capability` — przy `supported` źródło **MUST** podać drugi port w `server_port` odpowiedzi M6; sink SHOULD wysyłać RTCP (RFC 3550) — adaptacja bitrate do jakości łącza.
- §2.9 High-fidelity color space conversion (`microsoft_color_space_conversion`, 4:4:4 z powtarzanych klatek).
- §2.10 `microsoft_max_bitrate`: źródło „MUST encode the video stream at or below this bitrate".
- §2.11 `microsoft_multiscreen_projection` (primary/secondary + hres/vres/bitrate); §2.12 `microsoft_audio_mute`.
- Wymaganie ogólne (§2): sink implementujący rozszerzenia musi obsługiwać CABAC w Baseline i High; M bit w RTP zgodnie z RFC 2250 §3.3.
- Odporność sesji / discovery w MS-WFDPE: **brak** czegokolwiek o discovery, P2P, ani o czasach widoczności. Jedyne elementy „robustness": diagnostyka teardown (§2.2), RTCP (§2.8), IDR request capability (§2.6), format change bez M4 (§2.3).

---

## 7. Implikacje dla naszego źródła (ranking)

Legenda: [SPEC] = wymóg „shall"; [OPC] = opcjonalne/zalecane w spec; [WNIOSEK] = nasz domysł do zweryfikowania pomiarem.

1. **Terminy RTSP i M16 jako twarde minimum** [SPEC WFD §6.5, §6.5.1]: M1 ≤ 6 s po TCP; odpowiedź na każde żądanie od TV (także nieznane: `wfd_sec_view_mode`) w <5 s; następne żądanie ≤ 6 s po odpowiedzi (przed sesją); `Session: …;timeout=N` z N ≥ 10, M16 co < N−5 s. Test: zegar w logu dla każdego gap; alarm przy przekroczeniu. (Pokrywa TV sends sec_view_mode i fałszywe powody TEARDOWN od TV.)
2. **Serwer RTSP nasłuchuje zanim TV skończy 4-way** [SPEC WFD §6.5.2, §4.5.4]: TV inicjuje TCP i próbuje do 90 s. Sprawdzić, że restart `wfd-source`/`wfd-tx` nie zamyka gniazda na porcie z WFD IE; TCP accept ma działać w trakcie formacji grupy.
3. **Śledzenie bitów WFD Session Availability (B5:B4) i pozostałych bitów WFD IE TV w czasie** [SPEC WFD §4.5, §5.1.2; interpretacja WNIOSEK]: logować `wfd_dev_info` z każdego Probe Response/P2P-DEVICE-FOUND z timestampem; sprawdzić, czy „krótkie okno" to 0b01 przez kilka sekund, potem 0b00 (UI zapomina), albo TV w ogóle przestaje odpowiadać (wyszedł z Listen). Rozróżnia: „TV niedostępny logicznie" vs „TV poza Listen". Nie próbować GO Neg przy 0b00 (spec zabrania).
4. **Reagować na Probe Request/GO Negotiation od TV zamiast wyłącznie skanować** [SPEC P2P §3.1.2.1.1, §3.1.4.1; WNIOSEK]: TV w Smart View sam szuka (wysyła Probe Request na kanałach Social). Jeśli źródło jest w stanie Listen na wybranym Listen Channel (1/6/11) i odpowiada Probe Response, TV może zainicjować GO Neg w oknie, w którym go nie widzimy. Wniosek: krótkie okna Find (100–300 TU) na jednym kanale + częsty Listen na tym samym Listen Channel; Listen Channel najlepiej równy kanałowi STA, jeśli to 1/6/11 (mniej opuszczania kanału STA).
5. **Przy odkryciu TV: natychmiast GO Negotiation (nie czekać na kolejny skan)** [SPEC P2P §3.1.4.1: skan tylko Listen Channel celu; §3.1.4.2: 100 ms na odpowiedź; WNIOSEK]: przygotować wcześniej Channel List, WSC (PBC aktywne ≤120 s), GO Intent, kanał; PD Request PBC opcjonalnie na starcie. Cały cykl formacji ≤ 15 s [SPEC §3.1.4.1]. Częściej retry GO Neg po nieudanej wymianie (każda strona „may initiate Group Formation … again at a later time", §3.1.4.2) tak długo, jak TV jest widoczny.
6. **Kanał operacyjny = kanał STA (single channel)** [SPEC P2P §2.3, §3.1.4.2.2, §3.1.4.3; WNIOSEK o wykonaniu]: w Channel List i Operating Channel wysyłać tylko ten kanał; nie wymuszać zmiany kanału STA. Jeśli STA jest na kanale poza 1/6/11 (np. 2.4 GHz kanał 3), grupa i tak może tam działać (spec nie wymaga, by kanał operacyjny był Social); Listen Channel musi być Social (§3.1.2.1.1) i wtedy wymaga opuszczenia kanału STA na czas Listen/Find.
7. **Diagnostyka „FORMATION_FAILURE po WPS-REG-SUCCESS"** [WNIOSEK z P2P §3.1.4.3, §3.2.3, §3.1.4.1]: zalogować (a) czy po WPS-REG-SUCCESS GO nadal ma Group Formation=1 w Beaconie, (b) czy TV wysyła Probe Request/Auth/Assoc (monitor/`iw`) na kanale GO, (c) czas od WPS-REG-SUCCESS do porażki (porównać z 15 s i z timerami wpa_supplicant). TV, który nie wraca, jest po stronie klienta; po stronie źródła można: nie kończyć grupy zbyt wcześnie (poczekać na własny timeout wpa_supplicant), nie zmieniać kanału w trakcie, nie skanować (p2p_find/scan) w trakcie provisioning (nieobecność GO >100 ms = utrata ramek).
8. **Nie skanować ani nie robić Find, gdy trwa formacja lub sesja** [SPEC P2P §3.1.4.1: „shall stay on that channel until Group Owner Negotiation completes"; §3.1.2.4: GO poza kanałem ma ogłosić NoA]: wstrzymać `p2p_find`/skany STA od GO Neg Request do końca sesji; jeśli skan konieczny, NoA lub krótko.
9. **AP sieci współdzielonej rozłącza STA podczas Find** [WNIOSEK; spec P2P §3.4.3 tylko dla Managed AP]: zebrać reason code i obecność P2P IE/Minor Reason Code w Deauth/Disassoc; sprawdzić P2P Manageability w Beaconie AP (P2P §4.1.12). Jeśli Minor Reason Code = 1/2/3/4: spec radzi nie reasocjować, albo reasocjować z wyłączoną możliwością P2P (bez P2P IE) [SPEC §3.4.3 Note] — w wpa_supplicant odpowiada to `p2p_disabled=1` dla interfejsu STA (opcja implementacji, spec jej nie nazywa); ustawić też Cross Connection = 0 [SPEC §3.4.1]. Jeśli brak P2P IE w ramce: przyczyna poza spec P2P (np. ruch off-channel, nieaktywność, polityka AP) — ograniczyć czas poza kanałem STA (krótsze, rzadsze Find; unikać wielokrotnych pełnych skanów 1/6/11), zapewnić ruch na STA (keepalive/ARP) przed odjazdem na inny kanał [OPC/WNIOSEK].
10. **Odpowiedź na `wfd_idr_request`** [SPEC WFD §4.10.5, §6.4.13]: IDR+SPS+PPS „at the earliest opportunity" i 200 OK; limitować częstotliwość IDR nie wolno kosztem odpowiedzi. Mierzyć odstęp M13→IDR i częstość M13 (stały strumień M13 sugeruje utratę pakietów, np. przez nieobecność off-channel lub zbyt wysoki bitrate). Rozważyć `microsoft_max_bitrate`/RTCP, jeśli TV je ogłasza (PE §2.8, §2.10) [OPC].
11. **Unikać TEARDOWN/restartu strumienia bez potrzeby; preferować utrzymanie sesji** [SPEC WFD §4.12/§6.4.8: nowa sesja = pełne M1…M7; WNIOSEK z METODA zasada 7]: powtarzanie ostatniej klatki z ciągłym TS/PCR (≤100 ms) i M16; restart tylko kodera (już zrobione); przy `wfd_standby` od TV stosować §4.15 (nie TEARDOWN). Po błędzie przed M6: nie wysyłać M5 TEARDOWN, wrócić do Init i ponowić M1 [SPEC §6.4].
12. **Latency** [OPC PE §2.4]: jeśli M3 response zawiera `microsoft_latency_management_capability: supported`, wysłać `SET_PARAMETER … microsoft_latency_management_capability: low` (cel <50 ms po stronie sinka, „SHOULD"); dodać pole do M3 request tylko z nazwą parametru. Samsung najpewniej nie ogłasza (do sprawdzenia w M3). Decoder latency z `wfd_video_formats` jest tylko deklaracją.
13. **Grupa trwała / Invitation** [OPC P2P §3.1.5, §3.2.5; WFD §4.13]: wartość tylko jeśli TV (a) ogłasza Invitation Procedure (Device Capability bit 5), (b) zapamiętuje grupę, (c) jest w Listen. Test: po udanej sesji z `persistent` (autonomous GO z `persistent=1`) po zamknięciu wysłać `p2p_invite persistent=<id> peer=<TV>` w czasie, gdy TV jest w Smart View, i poza nim; mierzyć, czy TV odpowie Invitation Response w 100 ms (Status 0/1/8). Spec nie daje sposobu „obudzenia" TV bez UI.
14. **Rozdzielenie przyczyn „TV znika z Smart View po nieudanej formacji lub TEARDOWN"** [brak w spec.]: spec nie opisuje reakcji UI. Zbierać czas między zdarzeniem a ostatnim Probe Response TV (z punktu 3); po zdarzeniu czekać na ponowne zobaczenie TV (już zrobione) zamiast ponawiać GO Neg „w ciemno".
15. **Parametry vendor (`wfd_sec_view_mode`)** [SPEC §6.2/§6.4.4: ignorować nieznane, ale odpowiedzieć]: odpowiadać natychmiast 200 OK; nie traktować jako błędu sesji; nie zmieniać strumienia, jeśli nie rozumiemy (decyzja w profilu `sinks/`).
16. **Spójność WFD IE** [SPEC WFD §4.5.2.1, §4.4.2.2]: Source (Device Type 00), WFD R2 Device Information, Session Availability 01 — w Probe Request/Response, GO Neg Req/Resp/Conf, Assoc, Invitation i PD; zmiana treści w czasie formacji zabroniona („should be immutable") poza Availability.

### Pytania do pomiarów (rozstrzygną to, czego spec nie mówi)
- Czas, przez jaki TV odpowiada na Probe Request po wejściu w Smart View, oraz wartości bitów 5:4 i Device Capability (bit 5 Invitation, bit 2 Concurrent) w jego Probe Response.
- Interwał i kanał, na którym TV wysyła własne Probe Request (do dopasowania naszego Listen Channel).
- Czy TV ogłasza P2P Client Discoverability i Persistent Reconnect.
- Czy M3 od TV zawiera `microsoft_latency_management_capability`, `wfd_idr_request_capability`, `microsoft_diagnostics_capability`, `intel_*`.
