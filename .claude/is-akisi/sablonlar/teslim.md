# {Başlık} — Doğrulama & Yayın

> İlgili: [plan.md](plan.md) · [phase-{N}.md](phase-{N}.md)

{İşin dışarıya ne değiştirdiği — tek paragraf.}

## A. Doğrulama (ÖNCELİK)

{Yayından önce yeşil olması gereken her şey. Doğrulanmamış hiçbir şey
main'e gitmez.}

```sh
make hepsi
```

{Gerekiyorsa: `make shader`, `make terminfo`, `make test-yaris`, `make duman`,
tek crate sınamaları.}

### Beklenen çıktı

{Ne görülmeli — ölçüm değiştiyse önceki/sonraki değer, `docs/OLCUMLER.md`'ye bağla}

### Doğrulama Checklist

- [ ] `make hepsi` yeşil
- [ ]

## B. Yayın (doğrulamadan SONRA)

{Phase'lerin `## Yayın Etkisi` bloklarından derlenir. Her adım bir şeritle
etiketlenir:}

- **`[oto]`** — `/ship` kapsıyor (doğrulama + commit + `main`'e push).
- **`[komut]`** — kopyasız çalıştırılabilir CLI bloğu (`tic -x assets/terminfo/bateri.terminfo`,
  `make kur`) ya da kullanıcının tetiklediği bir skill (`/measure` — ölçüm
  bekleyen iddialar).
- **`[elle]`** — insan kararı gereken iş (belge güncellemesi, ölçüm sayısının
  `docs/OLCUMLER.md`'ye işlenmesi, sürüm notu, notarization).

### B.1 {adım} `[komut]`

### Yayın Checklist

<!-- `/ship` bekleyen manuel adımları BU başlık altında arar. -->

- [ ]

## Geri Alma

{Her bileşen için geri alma adımı — commit revert; ayar şeması değiştiyse
eski anahtarın hâlâ okunduğunun doğrulanması; terminfo değiştiyse eski
`TERM`'e dönüş; belge geri alma.}
