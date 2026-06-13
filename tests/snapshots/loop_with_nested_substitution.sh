for f in *.txt; do
  grep "$(cat pattern.txt)" "$f" > "out/$f"
done
