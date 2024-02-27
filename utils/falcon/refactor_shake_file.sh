FILE=$1
sed -i 's/pub ctx: \*mut uint64_t/pub ctx: crate::shake::Ctx/g' $FILE
sed -i 's/ctx: 0 as \*mut uint64_t/ctx: crate::shake::Ctx { uninit: (), }/g' $FILE
sed -i 's/Copy, Clone//g' $FILE


