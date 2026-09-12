CARGO ?= cargo

# Prerequisite sırası yalnız seri make'te garantidir; -j altında "en ucuz kapı
# önce" ve "sürüm başta" sözü bozulur.
.NOTPARALLEL:
.PHONY: hepsi fmt clippy test shader duman terminfo test-yaris kur

# Definition of done. Homebrew rustc pin'li değil (rust-toolchain.toml bilinçli
# olarak yok): bir `brew upgrade` sonrası gelen clippy kırmızısını kod
# kırmızısından ayırmak için sürüm başta basılır.
hepsi:
	@rustc --version
	@$(MAKE) --no-print-directory fmt clippy test

fmt:
	$(CARGO) fmt --all -- --check

clippy:
	$(CARGO) clippy --workspace --all-targets -- -D warnings

test:
	$(CARGO) test --workspace

# Pencereyi açar, BT_RUN_SECONDS dolunca kare, hücre, glyph, kural çizgisi ve
# atlas yuvası sayısına bakar:
# kare=N hucre=K glif=G kural=R yuva=U/T yuk=smoke istek=I kapanis=clean \
#   profil=debug ornek=off pipeline=ok
# İlk dördünden (kare, hucre, glif, kural) BİRİ 0 ise kırmızı; `yuva`, `yuk`,
# `istek` ve `profil` kapı değil, sayaç ve etiket.
# `kare` ayrıca ÜST SINIRLI (bkz. app.rs IDLE_FRAME_LIMIT): boşta sıfır kareyi
# bozan değişikliğin belirtisi eksik kare değil FAZLA karedir. `yuva` bir kapı
# değil sayaç — eşiği ölçülmedi, ölçülmemiş sayı kapıya yazılmaz.
# `kapanis` KISMEN kapı: panik kolları kırmızı düşürür, kayıtlı borç olan iki
# kol düşürmez — bağlansaydı kapı bilinen bir borç yüzünden kırmızı düşerdi.
# `ornek=off` = ölçüm kapısı (BT_FRAME_STATS) kapalıydı; ölçüm jetonları o
# koşuda HİÇ basılmaz.
# Jetonların tam listesi ve sözleşmesi: app.rs Report::token_line; `kapanis`
# değerleri teardown_token'da, `insufficient` push_span'de.
# Başsız ortamda binary stdout'a "ATLANDI" basıp 78 ile çıkar; make bunu 2
# olarak döndürür — ayırt edici sinyal stdout metnidir, çıkış kodu değil.
# `cargo run` CARGO_TARGET_DIR'a saygı duyar ve çocuğun çıkış kodunu geçirir.
# `env -u`: kabukta ihraç edilmiş bir BT_SCROLL_TEST ya da BT_FRAME_STATS
# kapıyı SESSİZCE başka bir koşuya çevirirdi — `var_os` değere değil VARLIĞA
# bakıyor, yani `BT_SCROLL_TEST=` bile yükü seçer. O koşu `yuk=load` basıp
# exit 0 verir, `hucre`/`kural` yarısı ise hiç sınanmaz: kapı yeşil kalır ama
# iddia ettiğinden başka bir şeyi sınar. Kapı hermetik olmalı.
duman:
	env -u BT_SCROLL_TEST -u BT_FRAME_STATS BT_RUN_SECONDS=3 $(CARGO) run -q -p bateri

# build.rs'in yaptığını cargo'nun bayatlık takibini atlayarak koşturur;
# derleme reçetesi burada TEKRARLANMAZ.
shader:
	touch $(wildcard crates/bt-gpu/shaders/*.metal)
	$(CARGO) build -p bt-gpu

# Paylaşılan duruma (PTY okuyucu thread'i ↔ kare üreten taraf) dokunan
# değişikliklerde koşar. ThreadSanitizer nightly ister; araç zinciri pin'li
# değil (`rust-toolchain.toml` bilinçli yok, Homebrew rustc) ve nightly yok:
# yerine iki farklı zamanlama profili — önce yalnız race_* stresi,
# sonra ignore'lular DAHİL bütün takım tek thread'de. İkinci satır
# --include-ignored taşımasa `make test`in kopyası olurdu ve yarış
# sınamasını hiç koşmazdı. TSan nightly gelince BURAYA üçüncü satır olur.
test-yaris:
	$(CARGO) test --workspace -- --ignored race_
	$(CARGO) test --workspace -- --include-ignored --test-threads=1

# Girdisi henüz olmayan hedefler. Var olurlar ki `proje.md`'nin doğrulama
# tablosu var olmayan bir hedef adı taşımasın; koşarlarsa "henüz yok" deyip
# kırmızı düşerler, "geçti" demezler. make reçete hatasını 2 ile döndürür;
# ayırt edici sinyal stdout'taki "henüz yok" metnidir. Hedef gerçek olunca
# satırı sil, proje.md başındaki listeden de çıkar.
henuz_yok = @echo "henüz yok: $(1)"; exit 1

terminfo:
	$(call henuz_yok,assets/terminfo bir shell/TERM setiyle gelir)
kur:
	$(call henuz_yok,.app paketi bundle setiyle gelir)
