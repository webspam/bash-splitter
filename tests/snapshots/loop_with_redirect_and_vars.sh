for f in *.txt; do
  grep "$pattern" "$f" > "out/$f"
done
