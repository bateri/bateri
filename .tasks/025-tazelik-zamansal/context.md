# Tazelik zamansal olsun — Bağlam

## Mevcut Durum

Giriş satırının ızgarada **bastırılması** aynanın güncel olduğuna güveniyor ve
bunu bir **tazelik kapısı** sınıyor (`Session::frame`):

```
fresh = last_ink_in_row(ızgara, satır) == DockState::last_ink  &&  at_anchor
```

Yani ölçüt **içerik**: ızgaranın o satırdaki son mürekkebi ile aynanın son
mürekkebi aynı mı. Kapı düşerse iki şey birden olur:

1. `suppress_to` `None` kalır → satır ızgarada **çizilir**.
2. `caret_in_dock` `false` olur (`session.rs:2050`, koşul
   `suppress_to.is_some()`) → **caret de ızgaraya gider**.

İkincisi 012'de bilerek kuruldu ve gerekçesi sağlam: ayna gerçekten bayatsa
dock **eski** metni, ızgara **yeni** metni gösterir; caret'i dock'ta bırakmak
kullanıcının o an yazdığı satırı caret'siz bırakırdı. Kural "caret satırın
çizildiği yere gider" ve **doğru**. Bozuk olan kural değil, **verdikt**.

Kapının kendi gerekçesi de ölçülmüş ve gerçek: `bracketed-paste-magic`
yapıştırmayı `zle -U` ile kuyruğa geri basıyor, ZLE typeahead varken
redisplay'i atlıyor ve `line-pre-redraw` — dolayısıyla aynamız — bir sonraki
tuşa kadar hiç koşmuyor. Kapı olmasaydı ızgara gizlenir, dock eski metni
gösterir ve kullanıcı yazdığını **hiçbir yerde** görmezdi.

## Motivasyon

**İçerik, zamanın vekili ve vekil sızdırıyor.** Sorulmak istenen şey "ayna
güncel mi", sorulan şey "ayna ile ızgara aynı şeyi mi söylüyor". İkisi ancak
zsh satırı **olduğu gibi** çizdiğinde örtüşüyor; zsh karakteri
**dönüştürdüğünde** ayna taze olmasına rağmen kapı düşüyor.

Sınıfın adı kodda zaten yazılı: *"ayna **ham** tamponu taşıyor, ızgara ise
**çizilmiş** hâli tutuyor."* Üç örnek biriktirdi:

| örnek | ayna der | ızgara der | akıbet |
|---|---|---|---|
| TAB (`Ctrl-V` + Tab) | `'\t'` | `None` (boşluğa açılmış) | **kapandı** (2026-09-18, sekme mürekkepsiz sayıldı) |
| `^A` (ham kontrol) | `'\x01'` | `'A'` | bilinen sınır, `shell.rs`'te yazılı |
| `<hex>` | `'🥰'` | `'>'` | **açık** — bu setin konusu |

Her biri ayrı bir yamayla kapatıldı ya da kapatılamadı; **ortak kökü** hiç
ele alınmadı.

### Kanıt — kullanıcı bildirdi, ölçüldü (2026-09-22)

Ekran görüntüsü: ızgarada ters videolu `<0001f970>` ve **caret orada**,
dock'ta `> 🥰` ve caret yok. Kullanıcının cümlesi ölçütü de veriyor:
*"zsh'ın öyle yazmasında sorun yok o onun kapsamı, bizim docktan niye imleç
oraya gidiyor daha yazarken."*

`<hex>`'i **zsh yazıyor** ve bu setin konusu değil. Ölçüldü (`zsh -f`, rc
dosyası yok, saf pty, `LANG=en_US.UTF-8`): zsh `🥰` U+1F970 için
`ESC[7m<0001f970>ESC[27m` gönderiyor — ters videolu on ASCII hücresi.
Kapsamı dar ve ölçülü: `🎉 😀 📁 ❤ 漢 Ａ █` hepsi ham geçiyor, yani yalnız
zsh'in basılabilirlik tablosunun bilmediği yeni kod noktaları (U+1F970
Unicode 10'dan) bu yola giriyor. Bizde `<hex>` çizen kod yolu **yok**.

Yani kullanıcının gördüğü kusur tek: **yazarken caret dock'tan çıkıyor.**

## Ne biliyoruz

Tasarımın dayanacağı üç olgu:

1. **Ayna ile ızgara aynı bayt akışından, sıralı geliyor.** Tarayıcı OSC
   8133'ü akışın içinden çekiyor (`CLAUDE.md` → `bt-core`), yani "ayna
   geldi" ile "ızgara yazıldı" olaylarının **sırası** elimizde.
2. **Tuş vuruşu başına sıra belli:** kullanıcı tuşa basar → ZLE işler →
   `line-pre-redraw` kancası aynayı basar → ZLE redisplay'i yazar. Yani
   normal hâlde **önce ayna, sonra ızgara**.
3. **Bayat hâlde ayna hiç gelmiyor.** `bracketed-paste-magic` ölçümü tam
   bunu söylüyor: redisplay atlanıyor **ve** kanca koşmuyor; bir sonraki
   tuşta ikisi birden geliyor.

Nesil sayacı örüntüsünün emsali depoda var ve aynı şekle sahip:
`Session::observe_screen_clear` baytları **uygulamadan önce** sayıyor, kare
yolu sayacı `Term` kilidinin **altında** tüketiyor ve henüz hesaba
katılmamış bir nesil aynı karede kuralı eziyor (`CLAUDE.md`).
