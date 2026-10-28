//! Windows.Graphics.Capture ile tek bir pencereden kare akışı (Mac'teki SCStream karşılığı).

use std::thread::sleep;
use std::time::{Duration, Instant};

use windows::core::{Interface, Result};
use windows::Graphics::Capture::{Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Graphics::SizeInt32;
use windows::Win32::Foundation::{HMODULE, HWND};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_CPU_ACCESS_READ,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_SDK_VERSION,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::System::WinRT::Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;

/// RGBA, satır satır, boşluksuz.
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub fn is_supported() -> bool {
    GraphicsCaptureSession::IsSupported().unwrap_or(false)
}

pub struct WindowCapture {
    context: ID3D11DeviceContext,
    device: ID3D11Device,
    d3d: IDirect3DDevice,
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    pool_size: SizeInt32,
    staging: Option<(ID3D11Texture2D, u32, u32)>,
    software: bool,
    /// Pencere değişmeyince yeni kare gelmez; son kare saklanır.
    last: Option<Frame>,
}

impl WindowCapture {
    pub fn start(hwnd: HWND) -> Result<WindowCapture> {
        // Sanal makinelerde donanım D3D11 olmayabilir; o zaman yazılımsal WARP denenir.
        let (device, context, software) = match create_device(D3D_DRIVER_TYPE_HARDWARE) {
            Ok((device, context)) => (device, context, false),
            Err(_) => {
                let (device, context) = create_device(D3D_DRIVER_TYPE_WARP)?;
                (device, context, true)
            }
        };
        let dxgi: IDXGIDevice = device.cast()?;
        let d3d: IDirect3DDevice = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi)? }.cast()?;

        let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
        let item: GraphicsCaptureItem = unsafe { interop.CreateForWindow(hwnd)? };
        let pool_size = item.Size()?;
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(&d3d, DirectXPixelFormat::B8G8R8A8UIntNormalized, 2, pool_size)?;
        let session = pool.CreateCaptureSession(&item)?;
        // Windows 10 2004+; eskilerde yok, önemli değil.
        let _ = session.SetIsCursorCaptureEnabled(false);
        // Windows 11: yakalanan pencerenin (oyunun) etrafındaki sarı çerçeve çizilmesin. Windows
        // 10'da yok; çerçeve kalır ama yakalama çalışır.
        let _ = session.SetIsBorderRequired(false);
        session.StartCapture()?;

        Ok(WindowCapture { context, device, d3d, pool, session, pool_size, staging: None, software, last: None })
    }

    /// Donanım hızlandırması yoksa (ör. sanal makine) true.
    pub fn uses_software_renderer(&self) -> bool {
        self.software
    }

    /// Bekleyen en yeni kareyi okur. `timeout` içinde yeni kare gelmezse önceki kare döner
    /// (`fresh` = false); hiç kare gelmediyse `None`.
    pub fn latest_frame(&mut self, timeout: Duration) -> Result<Option<(&Frame, bool)>> {
        let deadline = Instant::now() + timeout;
        let mut newest: Option<Direct3D11CaptureFrame> = None;
        loop {
            match self.pool.TryGetNextFrame() {
                Ok(frame) => newest = Some(frame),
                Err(_) if newest.is_some() || Instant::now() >= deadline => break,
                Err(_) => sleep(Duration::from_millis(5)),
            }
        }
        let fresh = newest.is_some();
        if let Some(frame) = newest {
            let read = self.read(&frame)?;
            let content = frame.ContentSize()?;
            let _ = frame.Close();
            if content.Width != self.pool_size.Width || content.Height != self.pool_size.Height {
                // Pencere boyutu değişti: havuz yeni boyutla kurulur.
                self.pool.Recreate(&self.d3d, DirectXPixelFormat::B8G8R8A8UIntNormalized, 2, content)?;
                self.pool_size = content;
            }
            self.last = Some(read);
        }
        Ok(self.last.as_ref().map(|frame| (frame, fresh)))
    }

    fn read(&mut self, frame: &Direct3D11CaptureFrame) -> Result<Frame> {
        let content = frame.ContentSize()?;
        let access: IDirect3DDxgiInterfaceAccess = frame.Surface()?.cast()?;
        let texture: ID3D11Texture2D = unsafe { access.GetInterface()? };
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { texture.GetDesc(&mut desc) };

        let staging = match &self.staging {
            Some((staging, w, h)) if *w == desc.Width && *h == desc.Height => staging.clone(),
            _ => {
                let staging_desc = D3D11_TEXTURE2D_DESC {
                    Usage: D3D11_USAGE_STAGING,
                    BindFlags: 0,
                    CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                    MiscFlags: 0,
                    MipLevels: 1,
                    ArraySize: 1,
                    ..desc
                };
                let mut staging = None;
                unsafe { self.device.CreateTexture2D(&staging_desc, None, Some(&mut staging))? };
                let staging = staging.expect("staging texture");
                self.staging = Some((staging.clone(), desc.Width, desc.Height));
                staging
            }
        };

        let width = (content.Width.max(0) as u32).min(desc.Width);
        let height = (content.Height.max(0) as u32).min(desc.Height);
        let mut rgba = vec![0u8; (width * height * 4) as usize];
        unsafe {
            self.context.CopyResource(&staging, &texture);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
            let source = std::slice::from_raw_parts(mapped.pData as *const u8, (mapped.RowPitch * desc.Height) as usize);
            for y in 0..height as usize {
                let row = &source[y * mapped.RowPitch as usize..][..width as usize * 4];
                let out = &mut rgba[y * width as usize * 4..][..width as usize * 4];
                for (dst, src) in out.as_chunks_mut::<4>().0.iter_mut().zip(row.as_chunks::<4>().0) {
                    dst[0] = src[2];
                    dst[1] = src[1];
                    dst[2] = src[0];
                    dst[3] = 255;
                }
            }
            self.context.Unmap(&staging, 0);
        }
        Ok(Frame { width, height, rgba })
    }
}

fn create_device(driver: D3D_DRIVER_TYPE) -> Result<(ID3D11Device, ID3D11DeviceContext)> {
    let mut device = None;
    let mut context = None;
    unsafe {
        D3D11CreateDevice(
            None,
            driver,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )?;
    }
    Ok((device.expect("D3D11 cihazı"), context.expect("D3D11 context")))
}

impl Drop for WindowCapture {
    fn drop(&mut self) {
        let _ = self.session.Close();
        let _ = self.pool.Close();
    }
}
