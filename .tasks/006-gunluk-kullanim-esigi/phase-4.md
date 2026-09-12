# Phase 4 — Bundle: `.app`, `make kur`, attribution

## Özet

bateri Dock'tan açılan, öne çıkabilen bir uygulama olur; lisans borcu kapanır.

_Requirements: R4, R4.1, R4.2, R4.3, R6.1, R6.2_

---

## 1. En küçük çalışan bundle

`crates/bateri/` + `Makefile` + `assets/` — `Info.plist`, ikon, gerçek
`make kur` (bugün `Makefile:76` "henüz yok"). `BT_RUN_SECONDS` yolu
bundle'lı açılışta da aynen çalışır; duman reçetesi değişmez.

Girmeyenler (Karar 6): imza, notarization, Sparkle. İmzasız `.app` ilk
açılışta Gatekeeper'a takılır — **tek seferlik** onay, her açılışta sağ-tık
değil. Bu gerçek eşik tanımına yazılır.

## 2. Attribution içerik denetimiyle

002'nin borcu: `alacritty_terminal` Apache-2.0, "lisans metni ve attribution
paneli **bundle**" (`.tasks/002-vt-motoru/teslim.md:54-56`). Eksik kalırsa
hiçbir kapı kızarmaz — sessiz lisans ihlali (işletme 3). O yüzden bundle
fazına **içerik denetimi** konur: Info.plist + lisans dosyası varlığı.

---

## Uygulama Notları

## Yayın Etkisi

- **app bundle** (`proje.md` → Yayın etkisi): `Info.plist`, ikon, `make kur`
  gerçek olur. Kullanıcının makinesinde ilk `.app` belirir.
- `Makefile`'daki `kur` satırı + proje.md başındaki "henüz yok" listesinden
  çıkarılır.
- Yeni bağımlılık yok. Ölçüm bekleyen iddia yok.

---

## Checklist

- [ ] `Info.plist` + ikon + gerçek `make kur`
- [ ] Attribution: lisans metni + panel; içerik denetimi (dosya varlığı)
- [ ] `BT_RUN_SECONDS` yolu bundle'lı açılışta çalışıyor
- [ ] Test: içerik denetimi (Info.plist + lisans varlığı)
- [ ] `[elle]` göz kontrolü: Dock'tan aç, öne çık, Gatekeeper tek seferlik onayı gör
- [ ] Doğrulama geçti (`proje.md` → Doğrulama; `make duman` **zorunlu**)
- [ ] `/simplify` çalıştırıldı, bulgular uygulandı
- [ ] `/code-review` çalıştırıldı, bulgular giderildi
- [ ] `/audit` çalıştırıldı, bulgular giderildi
- [ ] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [ ] Commit: {hash}
