source /usr/share/cachyos-fish-config/cachyos-config.fish

# overwrite greeting
# potentially disabling fastfetch
#function fish_greeting
#    # smth smth
#end

# bun
set --export BUN_INSTALL "$HOME/.bun"
set --export PATH $BUN_INSTALL/bin $PATH
function ffmpeg; env LD_LIBRARY_PATH=/usr/local/lib /usr/local/bin/ffmpeg $argv; end
function ffprobe; env LD_LIBRARY_PATH=/usr/local/lib /usr/local/bin/ffprobe $argv; end
