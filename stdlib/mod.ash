(a, b)

# a % b

# constants could be evaluated at compile time allowing
# non-field operations like floored division to be used
#
# for tasm execution an assembly implementation would be provided
#const quotient = in \ b
const quotient = 0
const remainder = a - quotient * b

assert_eq(lt(remainder, quotient), 1)
assert_eq(lt(quotient, quotient), 1)

assert_eq(divisor * quotient + remainder, dividend)

return remainder
