# Fare raporlama — Doğrulama & Yayın

> İlgili: [plan.md](plan.md) · [phase-1.md](phase-1.md) · [phase-2.md](phase-2.md)

Fare isteyen uygulama artık fareyi alıyor: kip açıkken (1000/1002/1003)
düğmenin basışı, bırakması ve hareket uygulamaya rapor olarak gidiyor,
kodlaması uygulamanın seçtiği (1006 SGR / 1005 UTF-8 / X10). Kullanıcının
metin seçme yeteneği **Shift** ile duruyor. Motive eden belirti — Claude
Code'un giriş kutusunda tıklanan yere imlecin gelmemesi — phase-1'de kapandı.
Yeni ayar anahtarı, yeni bağımlılık, türetilmiş dosya ve ölçüm iddiası
**yok**; dışarıya bakan tek belge değişikliği `CLAUDE.md` ile
`docs/YOL-HARITASI.md`'de ve ikisi de kendi commit'lerinde indi.

## A. Doğrulama (ÖNCELİK)

```sh
make hepsi
make duman
```

`make shader`, `make terminfo`, `make kur` ve `make test-yaris`
**gerekmiyor**: `.metal` ve `build.rs` değişmedi, `assets/` klasörlerine
dokunulmadı, yeni paylaşılan durum doğmadı (kısmanın çentiği `ViewIvars`'ta
ve yalnız ana thread'den görülüyor).

### Beklenen çıktı

`make hepsi` yeşil; `make duman` jeton satırında `hucre=8 glif=6 kural=15`,
`hareket > 0`, `kapanis=clean`.

**Duman'ın görünür pencere istediği bu sette ölçüldü** (phase-2 Uygulama
Notları): ekran uykudayken pencere çizilmiyor, `CAMetalDisplayLink` kare
vermiyor ve kapı `hareket=1` ile "animasyon yerleşmedi" diye kırmızı düşüyor.
Aynı düşüş phase-1 commit'inde de üretildi (`git stash`), yani belirti koda
değil ortama ait. Kırmızı görünce **önce ekranın açık olduğunu doğrula**.

Duman bu sette bir **regresyon alarmıdır, kapsama kapısı değil**: duman
kabuğu sabit bir betik, hiçbir fare kipi açmıyor ve depoda fare olayı
enjekte eden kanca yok — bu yol tamamen ölü olsa da yeşil düşerdi. Gerçek
doğrulama gözle kontroldü ve kullanıcı iki phase'i de doğruladı
(2026-09-21).

### Doğrulama Checklist

- [x] `make hepsi` yeşil
- [x] `make duman` yeşil (ekran açıkken)
- [x] Gözle kontrol, phase-1: Claude Code'da tıklama imleci taşıyor ·
      Shift+sürükleme metin seçiyor · `vim` + `:set mouse=a` · kip kapalı
      kabukta seçim bugünkü gibi
- [x] Gözle kontrol, phase-2: `vim`'de `mouse=a` ile sürükleyerek seçim ·
      `htop` · ham rapor (`printf '\033[?1003h\033[?1006h'; cat -v`) ·
      boşta pencerede fare gezdirmek kare üretmiyor
- [x] Set kapısı: `/code-review` (on bulgu, sekizi düzeltildi) + `/audit`
      (`make denetim` temiz, dört mercek temiz, ikisi ilgisiz)

## B. Yayın (doğrulamadan SONRA)

### B.1 Commit'leri `main`'e gönder `[oto]`

Üç commit ve hepsi kendi defterini taşıyor:

| commit | ne |
|---|---|
| `9009df4` | phase-1 — düğme raporu ve Shift arbitrajı |
| `e942022` | phase-2 — hareket raporu ve kısma |
| `d222223` | kapı — `/code-review` bulguları |

`/ship` bunları doğrulayıp gönderir; `.tasks/README.md`'nin 🟢'sı da onun
işi.

### B.2 Kullanıcıya söylenecek tek şey `[elle]`

**Shift+sürükleme.** Fare kipi açık bir uygulamanın (vim, htop, Claude Code)
içinde metin seçmenin **tek** yolu o, ve `docs/AYARLAR.md`'de davranış
bölümü olmadığı için yazılı olduğu tek yer `CLAUDE.md`. Bilinçli ve yazılı
bir sınır: ayar anahtarı eklemek geri alınamaz ("bilinmeyen anahtar asla
silinmez") ve emsal terminallerin hepsi anahtarsız.

### Yayın Checklist

- [ ] `/ship` — üç commit `main`'e, indeks 🟢

## Geri Alma

Üç commit de `git revert` ile geri alınabilir ve **sıra tersten**:
`d222223` → `e942022` → `9009df4`. Bağımlılık zinciri yalnız kod içinde;
kalıcı yan etki yok.

- **Ayar şeması:** dokunulmadı, geri alınacak anahtar yok.
- **Türetilmiş dosya:** yok.
- **Ölçüm:** `docs/OLCUMLER.md` değişmedi.
- **Belge:** `CLAUDE.md`'nin fare paragrafı ile `bt-core` satırı, ve
  `docs/YOL-HARITASI.md`'de iki borç maddesi (fare raporlamasının geri
  kalanı → daraldı; jest durumunun sınanamazlığı → yeni). Revert ikisini de
  eski hâline döndürür.
- **Davranış:** revert sonrası fare kipinde tıklama yine seçim başlatır,
  yani 020 öncesi hâl. Kullanıcının kaybettiği bir yetenek yok — Shift
  kaçış yolu yalnız kip açıkken anlamlı.
