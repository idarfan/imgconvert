//! 音量波形：把整條音軌（單聲道 8 kHz、16-bit）切成每 10 毫秒一格，記下每格的峰值與 RMS（平均音量）。
//! 純計算、不開程序，方便測試；實際讀音訊在 ffmpeg.rs 的 `waveform`。

/// 解碼波形用的取樣率（只看音量，不需要高音質）
pub const RATE: u32 = 8000;
/// 每秒幾格
pub const BINS_PER_SEC: f64 = 100.0;
const PER_BIN: usize = (RATE as f64 / BINS_PER_SEC) as usize;

/// 一格的音量：峰值、RMS，都在 0.0–1.0
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Bin {
    pub peak: f32,
    pub rms: f32,
}

/// 一邊讀樣本一邊累積成格子，不需要把整條音軌放在記憶體
#[derive(Default)]
pub struct Builder {
    peak: f32,
    sum_sq: f64,
    n: usize,
    bins: Vec<Bin>,
}

impl Builder {
    pub fn push(&mut self, sample: i16) {
        let v = (sample as f32 / 32768.0).abs();
        self.peak = self.peak.max(v);
        self.sum_sq += (v as f64) * (v as f64);
        self.n += 1;
        if self.n == PER_BIN {
            self.flush();
        }
    }

    /// 讀進一段 s16le 位元組；呼叫端要保證長度是偶數
    pub fn push_bytes(&mut self, bytes: &[u8]) {
        for pair in bytes.as_chunks::<2>().0 {
            self.push(i16::from_le_bytes(*pair));
        }
    }

    fn flush(&mut self) {
        let rms = (self.sum_sq / self.n.max(1) as f64).sqrt() as f32;
        self.bins.push(Bin { peak: self.peak, rms });
        (self.peak, self.sum_sq, self.n) = (0.0, 0.0, 0);
    }

    /// 結束：最後不滿一格的樣本也算一格
    pub fn finish(mut self) -> Vec<Bin> {
        if self.n > 0 {
            self.flush();
        }
        self.bins
    }
}

/// 時間 [t0, t1) 內的最大峰值與最大 RMS；時間軸上每一個像素欄呼叫一次
pub fn column(bins: &[Bin], t0: f64, t1: f64) -> Bin {
    if bins.is_empty() {
        return Bin::default();
    }
    let last = bins.len() - 1;
    let i0 = ((t0.max(0.0) * BINS_PER_SEC) as usize).min(last);
    // 一個像素比一格還窄時，至少取一格
    let i1 = ((t1.max(0.0) * BINS_PER_SEC).ceil() as usize).clamp(i0 + 1, bins.len());
    bins[i0..i1].iter().fold(Bin::default(), |a, b| Bin { peak: a.peak.max(b.peak), rms: a.rms.max(b.rms) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_one_bin_per_10ms() {
        let mut b = Builder::default();
        // 第一格：全部 0.5；第二格：一個 -1.0 其他 0；第三格只有 3 個樣本
        (0..PER_BIN).for_each(|_| b.push(16384));
        b.push(-32768);
        (1..PER_BIN).for_each(|_| b.push(0));
        (0..3).for_each(|_| b.push(8192));
        let bins = b.finish();
        assert_eq!(bins.len(), 3);
        assert_eq!(bins[0], Bin { peak: 0.5, rms: 0.5 });
        assert_eq!(bins[1].peak, 1.0);
        assert!((bins[1].rms - (1.0 / PER_BIN as f32).sqrt()).abs() < 1e-6);
        assert_eq!(bins[2], Bin { peak: 0.25, rms: 0.25 });
    }

    #[test]
    fn push_bytes_is_little_endian() {
        let mut b = Builder::default();
        b.push_bytes(&[0x00, 0x40, 0x00, 0xC0]); // 16384, -16384
        assert_eq!(b.finish(), vec![Bin { peak: 0.5, rms: 0.5 }]);
    }

    #[test]
    fn column_takes_max_and_handles_edges() {
        let bins: Vec<Bin> = (0..10).map(|i| Bin { peak: i as f32 / 10.0, rms: 0.1 }).collect();
        // 0.02–0.05 秒 = 第 2、3、4 格
        assert_eq!(column(&bins, 0.02, 0.05).peak, 0.4);
        // 比一格還窄：取那一格
        assert_eq!(column(&bins, 0.031, 0.032).peak, 0.3);
        // 超出結尾：取最後一格
        assert_eq!(column(&bins, 5.0, 6.0).peak, 0.9);
        assert_eq!(column(&[], 0.0, 1.0), Bin::default());
    }
}
