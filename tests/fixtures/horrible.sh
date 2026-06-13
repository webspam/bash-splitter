#!/usr/bin/env bash
# A deliberately abhorrent parser/splitter stress fixture.

# top-level pipelines
LD_PRELOAD=./evil.so grep -E "`whoami`-$(id -un)" /var/log/sys.log | (sort -u) | wc -l > counts.txt

( echo "start $(date +%s)"; cat `ls *.cfg` ) | tee combined.log | gzip -9 > combined.gz

producer | { read -r first; echo "$first" | tr a-z A-Z; } | cat -n

# one large control-flow command
for d in `find . -type d` $(ls -d /tmp/*/); do
    if [ -n "$d" ] && [[ "$d" =~ ^/.*$ ]]; then
        while read -r line; do
            case "$line" in
                *.log)
                    LD_PRELOAD=hook.so grep -E "`echo "$(basename "$d")"`" "$line" \
                        | sort \
                        | uniq -c \
                        > "/tmp/out-`echo "$d" | md5sum | cut -d' ' -f1`.txt"
                    ;;
                *)
                    ( cd "$d" && { tar cf - . | gzip > "$d.tgz"; } )
                    ;;
            esac
        done < <(cat "$d"/*.log 2>/dev/null)
    elif [ "`id -u`" -eq 0 ]; then
        for i in $(seq 1 `nproc`); do
            echo "spawning $i" && nohup worker --id="$i" --root="`pwd`" &
        done
    else
        until ping -c1 "`hostname -f`" > /dev/null 2>&1; do
            sleep $(( RANDOM % 5 ))
        done
    fi
done
