# Odak sorgusu

## Hedef

Dışarıdaki bir süreç (evlat), elindeki pane UUID'si için bateri'ye "kullanıcı
bu pane'e bakıyor mu, ona en son ne zaman dokundu" diye sorabilsin — yalnız o
pane için, izin istemeden, olay anında. Gerekçe ve kararlar
`discussion.md` → Karar 1–9.

## Gereksinimler

- **R1** — `bateri focus [--pid P] bateri://tab/<UUID>` tek jeton satırı basar
  ve GUI açmaz.
  - **R1.1** — `pane=live focused=0|1 idle=N`, `pane=none` ya da
    `pane=unknown`; `unknown` ayrı bir çıkış kodu; bozuk argüman tanı satırı ve
    kullanım hatası kodu (Karar 2, 8).
  - **R1.2** — `--pid` sahibi o pid olan tek örneğe sorar; yoksa sahibi canlı
    bütün örneklere, `pane=live` diyen ilki kazanır (Karar 5).
- **R2** — `focused=1` ⇔ bateri etkin, pane'in penceresi key ve pencerenin
  odaktaki pane'i bu pane (arama alanı dahil); bölmede odakta olmayan pane 0
  (Karar 3).
- **R3** — `idle` = pane'in son girdisinden (tuş, basış, tekerlek, fare
  hareketi, penceresinin key olması) beri geçen **tam saniye**, aşağı
  yuvarlanmış; girdisiz pane'de doğumdan; uykuyu sayan saatle (Karar 4, 6).
- **R4** — Tel: istek `focus 1 <UUID>\n`, cevap örneğin ürettiği jeton
  satırı; yalnız sorulan UUID cevaplanır, başka pane, başlık ya da liste
  hiçbir yoldan çıkmaz (Karar 2, 8).
- **R5** — Sınırlar: sunucuda bağlantı başına okuma ve ana kuyruğu bekleme
  sınırı, CLI'de örnek başına connect/read sınırı ve toplam son tarih; hiçbir
  istemci dinleyiciyi kilitleyemez, cevapsız örnek CLI'yi asamaz (Karar 8).
- **R6** — Ömür: dinleyici `Masters`'ın örnek dizininde (`bases()`'in ilki),
  süreli koşuda yok; ölü örneğin süpürmesi ve ⌘Q dizini soketiyle birlikte
  kaldırır (Karar 5).
- **R7** — `-` ile başlamayan tanınmayan argv[1] GUI açmadan sıfırdan farklı
  kodla çıkar (Karar 9).
- **R8** — `CLAUDE.md` sözleşmeyi tek cümle + işaretçi ile taşır.

## Yaklaşım

1. `bt-shell-common`'a platformsuz bir `focus` modülü: tel (istek/cevap
   biçimi, jeton satırı), sunucu (dinleyici + bağlantı başına sınır; cevabı
   enjekte edilen bir cevaplayıcıdan alır), istemci (örnekleri bul, sor,
   sınırla) ve uykuyu sayan saat. `ssh_route`'ta canlı örnek sayımı `sweep`
   ile paylaşılır, soket adı `remove_instance`'ın kümesine girer.
2. `bt-shell-macos`: pane'e son girdi damgası (`note_interaction`), ana
   kuyruğa sorup sınırla bekleyen cevaplayıcı, dinleyicinin `masters()`'ta
   kurulması, `bateri focus` girişi; `main.rs`'de alt komut ve tanınmayan
   alt komut kuralı; `CLAUDE.md`.

## Kapsam Dışı

- evlat tarafı (çağrı, sürüm kapısı, eşikler) — o deponun işi; sözleşme
  `context.md`'nin sonunda.
- Push / bildirim, `?1004`, OSC 9 bildirimleri, sistem geneli boşta süre.
- Linux kabuğunda cevaplayıcı (`bt-shell-linux` yok); modül Linux'ta
  derleniyor ve sınanıyor.

## Akış

```
evlat ──exec──▶ bateri focus --pid P bateri://tab/U        (GUI yok)
                  │ live_instances(roots) ∩ owner == P
                  ▼
        <örnek dizini>/focus  ──"focus 1 U\n"──▶  dinleyici thread'i
                                                    │ ana kuyruğa sor, sınırla bekle
                                                    ▼
                                   ana thread: pane_by_tab(U)?
                                     focused = active && key && focused_pane == pane
                                     idle    = ⌊now − last_input⌋ s
                  ◀── "pane=live focused=1 idle=4\n" ──┘
   zaman aşımı / bozuk tel → "pane=unknown" (ayrı çıkış kodu)
```

## Durum

| Phase | Durum |
|-------|-------|
| phase-1 | |
| phase-2 | |
| kapı | |
