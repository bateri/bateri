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

# Pencereyi açar, BT_RUN_SECONDS dolunca kare sayısına bakar: 0 → kırmızı.
# Başsız ortamda binary stdout'a "ATLANDI" basıp 78 ile çıkar; make bunu 2
# olarak döndürür — ayırt edici sinyal stdout metnidir, çıkış kodu değil.
# `cargo run` CARGO_TARGET_DIR'a saygı duyar ve çocuğun çıkış kodunu geçirir.
duman:
	BT_RUN_SECONDS=3 $(CARGO) run -q -p bateri

# build.rs'in yaptığını cargo'nun bayatlık takibini atlayarak koşturur;
# derleme reçetesi burada TEKRARLANMAZ.
shader:
	touch $(wildcard crates/bt-gpu/shaders/*.metal)
	$(CARGO) build -p bt-gpu

# Girdisi henüz olmayan hedefler. Var olurlar ki `proje.md`'nin doğrulama
# tablosu var olmayan bir hedef adı taşımasın; koşarlarsa "henüz yok" deyip
# kırmızı düşerler, "geçti" demezler. make reçete hatasını 2 ile döndürür;
# ayırt edici sinyal stdout'taki "henüz yok" metnidir. Hedef gerçek olunca
# satırı sil, proje.md başındaki listeden de çıkar.
henuz_yok = @echo "henüz yok: $(1)"; exit 1

terminfo:
	$(call henuz_yok,assets/terminfo bir shell/TERM setiyle gelir)
test-yaris:
	$(call henuz_yok,ThreadSanitizer nightly ister ve paylaşılan durum PTY setiyle gelir)
kur:
	$(call henuz_yok,.app paketi bundle setiyle gelir)
