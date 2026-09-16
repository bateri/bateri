# Phase 4 — Betik ürüne girer ve girdiği denetlenir

## Özet

Sarmalayıcı `.app` paketine kopyalanır ve kopyalandığı **denetlenir**; eksik
betiğin belirtisi, tasarlanmış sessizlikle karışmaz.

_Requirements: R6.1_

## Değişiklikler

- **`Makefile` → `kur`** — iki satır, ikisi de zorunlu: `assets/shell/`
  paketin `Contents/Resources`'ına kopyalanır **ve** içerik denetimine bir
  kalem eklenir (betik pakette var mı, girdiyle aynı mı). Emsal `Credits.html`
  ile `THIRD-PARTY-LICENSES.txt`'in `cmp -s` satırı. Kopya ile denetim iki
  ayrı elle yazılmış liste; biri eklenip öteki unutulursa kapı yine kör kalır.
- **`crates/bateri/src/bundle_assets.rs`** — girdi denetimi: betik `assets/`
  altında var ve okunabilir. Lisans metinlerinin denetimiyle aynı örüntü;
  `make hepsi`'de ajansız koşar.
- **`Makefile` → `denetim`** — rc dosya adı listesi genişletilir:
  `.zshenv`, `.zlogin`, `.zlogout` bugün listede **yok**, oysa bu setin
  yönlendirdiği dosyalar tam olarak onlar. Kapı, "betik kullanıcının rc
  dosyasına yazıyor mu" sorusunu ancak adı listede olan dosya için soruyor.
- **`.claude/is-akisi/proje.md`** — doğrulama tablosuna `assets/shell/*`
  satırı: betik değişince `make kur` koşulur. Bugün böyle bir satır yok, yani
  betik değişikliği hiçbir koşullu komutu tetiklemiyor.
- **`CLAUDE.md`** — "Shell entegrasyonu" maddesi bugüne kadar üç kabuğu
  anlatıyordu; bugünkü hâl (zsh indi, bash/fish sonraki sette) ve komut
  durumunun nerede yaşadığı yazılır.

**Neden ayrı phase:** eksik betiğin belirtisi ile tasarımın bilerek sessiz
yaptığı geri düşüş **aynı** — blok yok, hata yok. Üstelik betiğin debug'da
depo yolundan bulunması, bozulabilen tek derlemeyi (release) hiçbir kapının
dokunmadığı tek derleme yapıyor. Bu phase o asimetriyi kapatıyor.

## Kabul

- `make kur` betiği pakete koyuyor ve koymazsa **düşüyor** (çıkış 2):
  betiği geçici olarak kopyalamayan bir tarifle denenip doğrulanır.
- `make hepsi` girdi eksikse düşüyor (`bundle_assets` sınaması).
- `make denetim` genişletilmiş listeyle hâlâ `temiz`; listeye eklenen
  dosyalardan birine yazan **kasıtlı** bir betikle kırmızı düştüğü gösterilir.
- Paketten açılan bir oturumda (`open ... bateri.app`) entegrasyon kuruluyor:
  betik paket yolundan bulunuyor, depo yolundan değil.

## Yayın Etkisi

**app bundle** — pakete yeni bir kaynak türü giriyor (`Contents/Resources`
altında shell betiği). `Info.plist`, entitlements ve imza etkilenmiyor; imza
zaten yok (006 Karar 6).

**shell entegrasyonu** — betiğin ürüne giden yolu bu phase'de kapanıyor;
kullanıcı rc dosyasına dokunulmuyor ve denetimin listesi artık zsh'in beş
dosyasını da kapsıyor.

Türetilmiş dosya yok: betik **kaynaktır**, üretilmez.

shader yok · terminfo yok · ayar şeması yok (anahtar phase-3'te) · tema yok ·
yeni bağımlılık yok.

## Checklist

- [ ] `make kur`: kopya + içerik denetimi
- [ ] `bundle_assets`: girdi denetimi
- [ ] `make denetim`: rc dosya listesi genişletildi
- [ ] `proje.md` doğrulama tablosu + `CLAUDE.md`
- [ ] Test: betiksiz paket `make kur`'u düşürüyor; rc'ye yazan betik
      `denetim`'i düşürüyor
- [ ] Doğrulama geçti (`make hepsi` + `make kur` + `make duman`)
- [ ] Yayın etkisi yazıldı
