#!/usr/bin/env bash
# Turn the raw VHS capture into the polished demo (intro, slowed demo, outro,
# and an 8-second GIF for the README).
#
#   ./scripts/demo-setup.sh
#   vhs demo/emutui.tape
#   ./demo/render-post.sh
#
# Requires ffmpeg and ffprobe.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RAW="${1:-${REPO_ROOT}/docs/demo-raw.mp4}"
OUT="${REPO_ROOT}/docs/demo.mp4"
GIF="${REPO_ROOT}/docs/demo.gif"

if [[ ! -f "${RAW}" ]]; then
  echo "Raw capture not found: ${RAW}" >&2
  echo "Run: vhs demo/emutui.tape" >&2
  exit 1
fi

TMP="$(mktemp -d)"
trap 'rm -rf "${TMP}"' EXIT

W="$(ffprobe -v error -select_streams v:0 -show_entries stream=width -of csv=p=0 "${RAW}")"
H="$(ffprobe -v error -select_streams v:0 -show_entries stream=height -of csv=p=0 "${RAW}")"
[[ -n "${W}" && -n "${H}" ]] || { echo "could not read video size" >&2; exit 1; }

font() {
  local style="$1"
  local path
  path="$(fc-match -f '%{file}' "DejaVu Sans Mono:${style}" 2>/dev/null || true)"
  if [[ -f "${path}" ]]; then
    printf '%s' "${path}"
    return
  fi
  for path in \
    /usr/share/fonts/truetype/dejavu/DejaVuSansMono-"${style}".ttf \
    /usr/share/fonts/TTF/DejaVuSansMono-"${style}".ttf; do
    if [[ -f "${path}" ]]; then
      printf '%s' "${path}"
      return
    fi
  done
  echo "DejaVu Sans Mono (${style}) not found; install fonts-dejavu" >&2
  exit 1
}
FONT="$(font Book)"
BOLD="$(font Bold)"

# 3-second title card.
ffmpeg -y -v error -f lavfi -i "color=c=0x0b0d12:s=${W}x${H}:d=3:r=25" -vf "\
drawtext=fontfile=${BOLD}:text='emutui':fontcolor=0xf2f4f8:fontsize=110:x=(w-text_w)/2:y=h*0.34,\
drawbox=x=(iw-300)/2:y=h*0.50:w=300:h=4:color=0xFF9900:t=fill,\
drawtext=fontfile=${FONT}:text='A terminal console for AWS Emulator':fontcolor=0x9aa4b2:fontsize=32:x=(w-text_w)/2:y=h*0.53,\
drawtext=fontfile=${FONT}:text='github.com/vinycalves/aws-emulator-tui':fontcolor=0xFF9900:fontsize=24:x=(w-text_w)/2:y=h*0.61,\
fade=t=in:st=0:d=0.4,fade=t=out:st=2.5:d=0.5" \
  -c:v libx264 -preset medium -crf 20 -pix_fmt yuv420p -an "${TMP}/intro.mp4"

# 3-second closing card.
ffmpeg -y -v error -f lavfi -i "color=c=0x0b0d12:s=${W}x${H}:d=3:r=25" -vf "\
drawtext=fontfile=${BOLD}:text='One Rust binary. MIT.':fontcolor=0xf2f4f8:fontsize=64:x=(w-text_w)/2:y=h*0.36,\
drawtext=fontfile=${FONT}:text='Browse 436 services  ·  drill into resources  ·  invoke any operation':fontcolor=0x9aa4b2:fontsize=28:x=(w-text_w)/2:y=h*0.47,\
drawtext=fontfile=${FONT}:text='Independent project  ·  not affiliated with AWS':fontcolor=0x6e7785:fontsize=22:x=(w-text_w)/2:y=h*0.53,\
drawtext=fontfile=${FONT}:text='github.com/vinycalves/aws-emulator-tui':fontcolor=0xFF9900:fontsize=24:x=(w-text_w)/2:y=h*0.60,\
fade=t=in:st=0:d=0.4,fade=t=out:st=2.5:d=0.5" \
  -c:v libx264 -preset medium -crf 20 -pix_fmt yuv420p -an "${TMP}/outro.mp4"

# VHS drops idle frames, so slow the capture down to a readable pace.
ffmpeg -y -v error -i "${RAW}" -vf "setpts=1.8*PTS,fps=25" -an \
  -c:v libx264 -preset medium -crf 20 -pix_fmt yuv420p "${TMP}/demo-slow.mp4"

printf "file '%s'\nfile '%s'\nfile '%s'\n" \
  "${TMP}/intro.mp4" "${TMP}/demo-slow.mp4" "${TMP}/outro.mp4" > "${TMP}/list.txt"
ffmpeg -y -v error -f concat -safe 0 -i "${TMP}/list.txt" \
  -c:v libx264 -preset medium -crf 20 -pix_fmt yuv420p -an "${OUT}"

# Short, readable GIF for the README (S3 preview + DynamoDB).
ffmpeg -y -v error -t 8 -i "${TMP}/demo-slow.mp4" -vf "\
fps=12,scale=1100:-1:flags=lanczos,\
split[s0][s1];[s0]palettegen=max_colors=128[p];[s1][p]paletteuse=dither=bayer" \
  -loop 0 "${GIF}"

echo "Wrote ${OUT}"
echo "Wrote ${GIF}"
ffprobe -v error -show_entries format=duration,size -of default=noprint_wrappers=1 "${OUT}"