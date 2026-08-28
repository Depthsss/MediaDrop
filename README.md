<p align="center">
  <img src="src/assets/mediadrop-logo-white.png" width="260" alt="MediaDrop">
</p>

# MediaDrop

MediaDrop, Windows için Tauri 2 tabanlı bir medya analiz ve indirme uygulamasıdır. YouTube, Instagram, X/Twitter ve TikTok içeriklerini mevcut `yt-dlp`, `gallery-dl`, FFmpeg ve ffprobe hatlarıyla işler. Opera/Chromium companion eklentisi aynı masaüstü motorunu kullanır; tarayıcı içinde ikinci bir indirme motoru çalıştırmaz.

## İndir

En güncel kurucu: [MediaDrop Releases](https://github.com/Depthsss/MediaDrop-Releases/releases/latest)

Windows kurucusu henüz Authenticode ile imzalı değildir. Windows SmartScreen “Bilinmeyen yayıncı” uyarısı gösterebilir. İndirdiğiniz dosyayı release sayfasındaki `SHA256SUMS.txt` ile doğrulayın. Uygulama içi güncellemeler ayrı Tauri updater imzasıyla doğrulanır.

## Sistem gereksinimleri

- Windows 10 22H2 veya Windows 11, x64
- Kurulum sırasında bir kez Windows yönetici onayı (UAC)
- WebView2 Runtime (çoğu güncel Windows kurulumunda hazırdır; eksikse gömülü bootstrapper indirme/yükleme başlatacağı için internet gerekir)
- Analiz ve indirme için internet bağlantısı

32-bit Windows ve native ARM64 paketleri 1.0.0 kapsamında değildir. DRM korumasını aşmaz ve desteklenen platformların erişim kurallarını değiştirmez.

## Özellikler

- YouTube video, MP3, kalite seçimi ve hızlı klip
- Instagram gönderi, Reel, Story ve carousel
- X/Twitter video, fotoğraf, metin, alıntılı gönderi ve PNG/MP4 gönderi kartı
- TikTok video ve fotoğraf carousel
- İlerleme, hız, pause/resume/cancel ve doğrulama
- Aynı anda tek aktif iş; gizli queue veya paralel ikinci motor yok
- Opera, Opera GX, Chrome ve Edge için MV3 companion
- Tamamlanan dosyayı doğrudan Explorer’da gösterme

## Tarayıcı eklentisi

Kurucudaki “Tarayıcı eklentisini bağla” seçeneği varsayılan olarak kapalıdır. Seçilirse MSI kurulumu bittiğinde kurucu kapanmadan kendi bağlantı ekranına geçer. Bu ekran Opera GX, Opera, Chrome ve Edge kurulumlarını tarar, varsayılan tarayıcıyı ilk sıraya alır, doğru eklenti sayfasını açar ve paketlenmemiş eklenti klasörünü panoya hazırlar. Görünür MediaDrop penceresi açılmaz; yalnız arka plandaki companion bağlantıyı doğrular ve eklenti yüklendiğinde kurucu otomatik olarak hazır durumuna geçer.

Eklentiyi daha sonra kurmak veya bağlantıyı onarmak için uygulamadaki **Eklentiyi kur veya onar** düğmesi aynı tarayıcıya özel adımları yeniden açar.

Eklenti cookie, Authorization header veya browser trafiği okumaz. Gerekli oturum izni masaüstü uygulamasında ve kullanıcı onayıyla yönetilir.

## Kaynaktan çalıştırma

Gerekenler: Windows 10 22H2/11 x64, Node.js 22, Rust stable MSVC, Python 3.10 ve PowerShell 5.1+.

```powershell
npm ci
powershell -NoProfile -ExecutionPolicy Bypass -File .\prepare-sidecars.ps1
npm run verify:frontend
cd src-tauri
cargo test --locked
cd ..
npm run tauri dev
```

`prepare-sidecars.ps1`, upstream release binary’lerini `sidecars.lock.json` içindeki SHA-256 değerleriyle doğrular; kaynakta tutulan Instagram helper’ı hash-kilitli Python bağımlılıklarıyla yerel olarak üretir. Üretilmiş `.exe` dosyaları Git’e eklenmez.

## Release

İlk public history’yi mevcut özel çalışma geçmişini değiştirmeden hazırlamak için:

```powershell
.\prepare-public-history.ps1
```

Bu komut ayrı bir private backup ve ayrı, tek commit’li `MediaDrop-Public-1.0.0` klasörü oluşturur; remote eklemez veya push yapmaz. Public repo bağlantısı ve ilk CI başarıyla tamamlandıktan sonraki yerel yayın akışı:

```text
release-mediadrop.bat
```

Script temiz/push edilmiş `main`, aynı commit için başarılı CI, sürüm eşitliği, dependency/secret kontrolleri, sidecar hash’leri, native-host origin’i, Windows uyumluluğu, MSI içeriği ve artefakt hash’leri geçmeden GitHub release yayımlamaz. Ayrıntılı ilk kurulum ve kurtarma adımları [docs/RELEASING.md](docs/RELEASING.md) içindedir.

## Mahremiyet ve güvenlik

- Kaynak URL, signed query, cookie ve tokenlar extension storage’a yazılmaz.
- Cloud hata raporu yeni kurulumlarda kapalıdır ve yalnız açık onayla etkinleşir.
- Native host yalnız sabit production extension origin’ini kabul eder.
- Güvenlik açıklarını public issue yerine [özel security advisory](https://github.com/Depthsss/MediaDrop/security/advisories/new) üzerinden bildirin.

## Lisans

MediaDrop kaynak kodu [MIT](LICENSE) lisanslıdır. Birlikte dağıtılan araçlar kendi lisanslarına tabidir; ayrıntılar [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) içindedir.
