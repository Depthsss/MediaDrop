# MediaDrop release rehberi

## Bir defalık hazırlık

1. `Depthsss/MediaDrop` public ve `Depthsss/MediaDrop-Releases` release depolarını oluşturun.
2. GitHub CLI ile giriş yapın: `gh auth login --web`.
3. Mevcut Tauri updater private key’ini `%USERPROFILE%\.tauri\mediadrop.key` konumunda saklayın. Bu dosyayı repository’ye kopyalamayın.
4. Node.js 22, stable Rust MSVC, Python 3.10, Git ve Windows App Installer (`winget`) kurun.
5. GitHub repository ayarlarında Private Vulnerability Reporting’i açın.

İlk public kaynak geçmişi için mevcut çalışma repository’sinde şunu çalıştırın:

```powershell
.\prepare-public-history.ps1
```

Script mevcut repository’yi yeniden yazmaz. Kardeş klasörde private Git bundle + working-tree yedeği ve ayrı, tek commit’li public kaynak klasörü oluşturur. Oluşan public klasörde `origin` değerini yalnız bir kez ekleyip `main` branch’ini push edin ve CI’ın geçmesini bekleyin.

## Normal yayın

Kaynak `main` temiz, remote ile eşit ve CI yeşilken:

```text
release-mediadrop.bat
```

Script eksikse Cargo Audit, Gitleaks, GitHub CLI ve NSIS için güvenli kurulum yolunu kullanır. Updater key parolasını ister; parola komut satırına veya loga yazılmaz. Tüm yerel kontroller geçince yalnız bir public yayın onayı ister.

Yerel build/doğrulama yapıp GitHub’a yüklememek için:

```powershell
.\release-mediadrop.ps1 -SkipPublish
```

Yalnız preflight için:

```powershell
.\release-mediadrop.ps1 -PreflightOnly
```

Yalnız mevcut MSI/signature’dan updater JSON üretmek için `generate-latest.bat` kullanılabilir.

## Üretilen artefaktlar

- `MediaDrop-Setup-1.0.0.exe`: son kullanıcı için branded kurucu
- `MediaDrop_1.0.0_x64.msi` ve `.sig`: Tauri updater girdisi
- `MediaDrop-Extension-1.0.0.zip`: manuel eklenti onarımı
- `latest.json`: updater endpoint’i
- `SHA256SUMS.txt`: public dosya hash’leri
- `build-info.json`: commit/tool/sidecar sürümleri
- `THIRD_PARTY_NOTICES.md`: dağıtılan bileşen lisansları

MSI Windows tarafından UAC ile per-machine kurulur; native-host ve uygulama yolu HKLM altında uninstall ile temizlenen sabit kayıtlarla bulunur. WebView2 mevcut değilse setup içindeki Microsoft bootstrapper internetten Evergreen Runtime yükler. Bu nedenle MediaDrop payload’ı offline olsa da WebView2’siz temiz makinede internet gerekir.

## Hata ve rollback

- Test, audit, hash, MSI extract veya compatibility kapısı başarısızsa public release oluşturulmaz.
- Draft oluşturulduktan sonra doğrulama başarısızsa draft public yapılmaz; GitHub’da incelenip silinebilir.
- Public release artefaktı değiştirilmez. Bir hata bulunursa `1.0.1` hazırlanır.
- MSI `upgradeCode`, app identifier, native-host adı ve production extension public key’i değiştirilmez.
- MSI içindeki `browser-extension` kaynağı Opera GX, Opera, Chrome ve Edge’de paketlenmemiş eklenti olarak yüklenebilir olmalıdır; kurucu mağaza adresine veya browser policy’sine bağlı değildir. Checkbox seçiliyse bağlantı rehberi görünür uygulamaya devredilmeden aynı kurucu penceresinde kalmalı ve native `hello` geldiğinde hazır durumuna geçmelidir.
