# Atlas tahliyesi — Doğrulama & Yayın

> İlgili: [plan.md](plan.md) · [phase-1.md](phase-1.md)

Atlas dokusunun kenarı sabit 1024 olmaktan çıkıp hedeflenen yuva sayısından
türüyor. Dışarıya değişen tek şey **büyük puntonun kapasitesi**: Retina'da
29pt'de atlas 406 yuva veriyordu ve yordamsal aile 422 istiyordu, yani aile
hiç sığmıyor ve o puntoda ilk kez görülen her karakter o oturumda kalıcı
kutu kalıyordu. Varsayılan punto **bit bit aynı**; `bt-gpu` hiç değişmedi.

## A. Doğrulama (ÖNCELİK)

```sh
make hepsi
make duman
```

### Beklenen çıktı

- `make hepsi` yeşil; `bt-atlas` 62 sınama.
- Set kapısı: `/code-review` 12 bulgu verdi, beşi kod beşi belge olarak
  düzeltildi, biri (kare-dışı büyüme) gerekçesiyle alınmadı —
  [phase-1.md](phase-1.md) → Uygulama Notları.
- `make duman`: `yuva=13/1984` — varsayılan puntonun kapasitesi
  **değişmemiş** olmalı. Değiştiyse `MIN_EDGE` ya da `SLOT_TARGET` oynamış
  demektir ve varsayılan kullanıcının rasteri kaymıştır.
- Kapasite sayıları `docs/OLCUMLER.md` → `## Atlas yuva ayak izi` (021'in
  ölçümü; bu set yeni sayı ölçmedi, ölçülmüş sayıyı **sözleşmeye** çevirdi).

### Doğrulama Checklist

- [x] `make hepsi` yeşil (kapı düzeltmelerinden sonra yeniden)
- [x] `make duman` yeşil ve `yuva=13/1984`
- [x] `crates/bt-gpu` diff'i boş (R5)
- [x] Değişmezin dişi doğrulandı (`MAX_EDGE` mutasyonu bekçiyi düşürdü,
      geri alındı)
- [~] `make kur` — **koşulu doğmadı**: `assets/bundle`, `assets/shell`,
      `crates/bateri` ve `kur` hedefi el değmedi
- [~] `make shader` / `make terminfo` / `make test-yaris` — koşulları doğmadı
      (`.metal`, `build.rs`, terminfo girdisi ve paylaşılan durum el değmedi)

## B. Yayın (doğrulamadan SONRA)

### B.1 `/measure` — sekme başına bellek `[komut]`

Doku 1 MB'dan **16 MB'a** kadar çıkabiliyor: ara kademe 29pt civarında
4 MB (2048 kenar), en kötü köşe (`MAX_POINT_SIZE` × `line_height` 2.0)
~16 MB (4096 kenar). Köşe **kullanıcının erişebildiği** bir yer — Retina'da
Cmd + ile 72pt'ye çıkmak punto × ölçeği tavana getiriyor. Ölçüm bu yüzden
4 MB'a değil 16 MB'a kurulmalı.
**Varsayılan puntoda fark sıfır bayt**, yani günlük kullanımda ölçülecek bir
şey yok; kalem büyük punto ve 025 (sekme + bölme) için açık.

`docs/OLCUMLER.md` → `## Bellek` bugün **boş** ve kancası yok (`footprint`,
`vmmap` dışarıdan), yani bu bir blokaj değil kayıtlı bir kalem. Ölçüm bir
kapı değildir.

```sh
/measure bellek
```

### Yayın Checklist

- [ ] B.1 `/measure` — sekme başına bellek; doku büyümesinin gerçek ayak izi
      ölçülüp `docs/OLCUMLER.md` → `## Bellek`'e işlendi

## Geri Alma

Tek adım: setin iki commit'ini revert et (`8dc580d` planlama, `aa007b8`
kod). Kod tek crate'te ve tek fonksiyonda (`edge_for`); `bt-gpu` hiç
dokunulmadığı için geri alma yüzeyi de tek taraflı.

- **Ayar şeması:** geri alınacak şey yok — yeni anahtar eklenmedi, hiçbir
  anahtarın kabul aralığı değişmedi.
- **Belge:** `CLAUDE.md`'nin `bt-atlas` satırı ve `docs/YOL-HARITASI.md`'nin
  borç maddesi aynı commit'te değişti, revert ikisini birlikte geri alır.
- **Kullanıcı verisi:** yok. Atlas süreç ömürlü, diskte karşılığı yok.
- **Dikkat:** revert kusuru da geri getirir — 29pt ve üstünde atlas yeniden
  ailenin altına düşer.
