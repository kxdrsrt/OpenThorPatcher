# App icon layers (Apple Liquid Glass)

Apple's HIG asks for **unmasked, full-bleed square layers** (1024x1024) that Icon Composer
turns into a Liquid Glass icon (specular highlight, refraction, dark/clear/tinted variants).
Rounded corners, shadows and highlights must not be baked in.

| File | Role |
| --- | --- |
| `1-background.png` / `.svg` | Opaque full-bleed gradient (set it as the background fill in Icon Composer) |
| `2-bolt.png` / `.svg` | Foreground layer 1, flat lightning shape (give it some translucency) |
| `3-hammer.png` | Foreground layer 2, the hammer, clean edges, transparent background |
| `4-badge.png` | Maker only, the gold plus badge |
| `preview-unmasked.png` | Flattened preview of the stack |

## Build the real `.icon`

1. Open Icon Composer (Xcode 26 or developer.apple.com/icon-composer), drop in the layers in the order above.
2. Export `OpenThorPatcher.icon`, then run `xcrun actool` or add it to an Xcode asset catalog.
3. For Tauri, put the resulting `.icns` in `src-tauri/icons/icon.icns`.

## What ships without Icon Composer

`src-tauri/icons/icon.icns` is a classic Big Sur style icon (824 px plate, soft shadow) composed from the same
layers. It is **not** a true Liquid Glass icon: the system effects only apply to a `.icon` built in Icon Composer.
