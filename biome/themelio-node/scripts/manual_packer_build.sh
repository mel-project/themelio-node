#!/bin/bash

set -ex

export SCRIPTS_DIRECTORY="$(dirname "${0}")"

mkdir -p ${SCRIPTS_DIRECTORY}/packer/temporary-templates

for region in $(cat $SCRIPTS_DIRECTORY/packer/aws_regions); do
  echo "Creating packer templates for the $region region."

  export AWS_REGION=${region}

  envsubst < "${SCRIPTS_DIRECTORY}/packer/mainnet.pkr.hcl.temp" > "${SCRIPTS_DIRECTORY}/packer/temporary-templates/mainnet-$region.pkr.hcl"
  envsubst < "${SCRIPTS_DIRECTORY}/packer/testnet.pkr.hcl.temp" > "${SCRIPTS_DIRECTORY}/packer/temporary-templates/testnet-$region.pkr.hcl"
done

cp ${SCRIPTS_DIRECTORY}/packer/00-base-image.pkr.hcl ${SCRIPTS_DIRECTORY}/packer/temporary-templates/

echo "Joining packer templates"
sed -e '$s/$/\n/' -s ${SCRIPTS_DIRECTORY}/packer/temporary-templates/*.hcl > ${SCRIPTS_DIRECTORY}/themelio-node-debian-aws.pkr.hcl.temp
envsubst < "${SCRIPTS_DIRECTORY}/themelio-node-debian-aws.pkr.hcl.temp" > "${SCRIPTS_DIRECTORY}/themelio-node-debian-aws.pkr.hcl"

echo "Cleaning up temporary files"
rm themelio-node-debian-aws.pkr.hcl.temp
rm -rf ${SCRIPTS_DIRECTORY}/packer/temporary-templates

echo "Validating packer template"
packer validate "${SCRIPTS_DIRECTORY}/themelio-node-debian-aws.pkr.hcl"


if [ -n "${DO_BUILD}" ]; then
  echo "Building packer images"

  packer build "${SCRIPTS_DIRECTORY}/themelio-node-debian-aws.pkr.hcl"
else
  echo "DO_BUILD not set, skipping packer build."
fi