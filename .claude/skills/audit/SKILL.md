---
name: audit
description: Değişen kodu projenin kendi kuralları için denetler — mekanik denetim komutunun üstüne projenin yargı mercekleri (proje profilinde). Kullanıcı "denetle", "kurallara uyuyor mu", "mimari bozuldu mu" dediğinde ve /implement'in set kapısında /code-review'dan SONRA koşar. Genel hata avı değildir.
allowed-tools: Read, Glob, Grep, Agent, Bash(git:*), Bash(ls:*), Bash(grep:*)
---

Değişen kodu **bu depoya özgü** kurallar için denetle. `/code-review` hata
avlar ve projeye özgü kural okumaz; bu skill yalnız o boşluğa bakar.

Mercekler ve mekanik denetim komutu `.claude/is-akisi/proje.md`'dedir
(Denetim mercekleri, Doğrulama); kuralların gerekçesi proje sözleşmesinde.
Bu skill yalnız akışı yürütür — mercek listesini gövdesinde taşımaz.

## Girdi

`$ARGUMENTS` bir hedef veriyorsa (commit aralığı, dosya, `.tasks/{set}`) onu
al; set verildiyse aralık `duzen.md` → Set aralığı. Vermiyorsa çalışma
ağacındaki değişiklik (`git diff HEAD`); o da yoksa son commit.

## Kurgu

**1. Mekanik yarı.** Projenin mekanik denetim komutu kapı komutunun içinde
her phase'de zaten koşuyor. Burada yeniden grep'leme; komutu bir kez koş ve
sonucunu rapora al.

**2. Eleme.** `git diff --name-only {hedef}` ile hangi merceğin ilgili
olduğunu belirle; değişmemiş alana bakan mercek düşer ve raporda
**"ilgisiz"** diye sayılır — sessizce düşen mercek "denetlendi" gibi okunur.

**3. İnline koş.** Mercekleri kendin koş. Fan-out yalnız üç ya da daha fazla
yargı merceği ilgiliyse **ve** diff büyükse (birkaç yüz satırı aşıyorsa)
anlamlıdır: her mercek için bir `Agent`, paralel, güçlü model, istemde otonom
şeridin Ajan kuralları; ajana yalnız kendi merceğini ve ilgili diff'i ver.
Sentez ana döngüdedir, devredilmez.

Her mercek için: bulgu varsa `dosya:satır` + neden ihlal + ne yapılmalı. Bulgu
yoksa tek satır "temiz".

## Çıktı

Önce mekanik denetimin sonucu, sonra bulgular önem sırasıyla: `dosya:satır` ·
mercek · tek cümle ihlal · önerilen düzeltme. Bulgu yoksa hangi merceklerin
temiz, hangilerinin ilgisiz olduğunu tek satırda say.

Bulgu **uydurma**: zorlama bulgu, gerçek bulguyu gömer.
