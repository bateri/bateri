# Phase 0 — Depoyu başlat

## Özet

`git init`, `.gitignore` ve bugün var olan belgelerin tek commit'i; kod yok —
sonraki phase commit'leri kök commit'in üstüne oturup tek tek geri alınabilsin.

_Requirements: R1_

---

## 1. `.gitignore`

`.gitignore`

```gitignore
# derleme çıktıları
/target/
*.metallib
*.air
*.dSYM/
# paketleme
*.app/
*.dmg
# macOS
.DS_Store
# ölçüm kayıtları
*.trace/
```

`Cargo.lock` **listelenmez**: uygulama deposudur, kilit dosyası girer
(`CLAUDE.md`, `proje.md` → Teslim).

## 2. Depo ve ilk commit

```sh
git init -b main
git add .gitignore CLAUDE.md .claude .tasks docs
git commit -m "Depoyu başlat"
```

Commit yalnız belgeleri taşır: `CLAUDE.md`, `.claude/`, `.tasks/`, `docs/`,
`.gitignore`. `.claude/worktrees/.gitkeep` de girer (dizin boş kalırsa git
onu düşürür).

---

## Uygulama Notları

## Yayın Etkisi

yok

---

## Checklist

- [ ] `.gitignore` yazıldı
- [ ] `git init -b main`, belgeler eklendi
- [~] Doğrulama geçti — cargo workspace henüz yok, `make hepsi` koşamaz; bu phase kod içermiyor
- [~] `/simplify` — kod yok
- [~] `/code-review` — kod yok
- [~] `/audit` — kod yok; `.gitignore` içeriği `proje.md` → Teslim listesiyle elle karşılaştırıldı
- [ ] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [ ] Commit: {hash}
