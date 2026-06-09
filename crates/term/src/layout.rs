//! 端末サイズとグリッド寸法から配置（センタリング / 最小サイズ未満ポーズ）を決める純粋関数。

/// グリッドの端末内配置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// 端末が最小サイズ未満。プレイを止めてリサイズを促す。
    TooSmall,
    /// グリッドを左上 `(ox, oy)` に置いてセンタリング（レターボックス）。
    Fit { ox: u16, oy: u16 },
}

/// 端末 `(tw, th)` にグリッド `(gw, gh)` を配置する。未満なら [`Layout::TooSmall`]。
pub fn compute_layout(term: (u16, u16), grid: (u16, u16)) -> Layout {
    let (tw, th) = term;
    let (gw, gh) = grid;
    if tw < gw || th < gh {
        Layout::TooSmall
    } else {
        Layout::Fit {
            ox: (tw - gw) / 2,
            oy: (th - gh) / 2,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_fit_is_top_left() {
        assert_eq!(
            compute_layout((64, 24), (64, 24)),
            Layout::Fit { ox: 0, oy: 0 }
        );
    }

    #[test]
    fn larger_terminal_centers() {
        assert_eq!(
            compute_layout((84, 30), (64, 24)),
            Layout::Fit { ox: 10, oy: 3 }
        );
    }

    #[test]
    fn too_narrow_or_short_is_too_small() {
        assert_eq!(compute_layout((63, 24), (64, 24)), Layout::TooSmall);
        assert_eq!(compute_layout((64, 23), (64, 24)), Layout::TooSmall);
        assert_eq!(compute_layout((0, 0), (64, 24)), Layout::TooSmall);
    }

    #[test]
    fn odd_margin_rounds_down() {
        // (65-64)/2 = 0, (25-24)/2 = 0
        assert_eq!(
            compute_layout((65, 25), (64, 24)),
            Layout::Fit { ox: 0, oy: 0 }
        );
    }
}
